use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

/// Collect the host facts used to describe a local run. On Windows the
/// values come from CIM/WMI rather than being guessed from environment
/// variables; no serial numbers or account identifiers are collected.
pub fn collect_hardware(instance_dir: Option<&Path>) -> Result<BTreeMap<String, String>> {
    if cfg!(windows) {
        match collect_with_powershell(instance_dir) {
            Ok(values) if !values.is_empty() => return Ok(values),
            Ok(_) => {}
            Err(error) => {
                let mut fallback = collect_fallback();
                fallback.insert("collection_warning".to_string(), error.to_string());
                return Ok(fallback);
            }
        }
    }
    Ok(collect_fallback())
}

fn collect_with_powershell(instance_dir: Option<&Path>) -> Result<BTreeMap<String, String>> {
    // Keep this script static. It is passed as one argument and never contains
    // a path or other user-controlled value.
    let script = r#"
$utf8NoBom = New-Object -TypeName System.Text.UTF8Encoding -ArgumentList $false
[Console]::OutputEncoding = $utf8NoBom
$OutputEncoding = $utf8NoBom
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$gpus = @(Get-CimInstance Win32_VideoController)
$activeGpu = $gpus |
  Where-Object {
    $_.CurrentHorizontalResolution -gt 0 -and $_.CurrentVerticalResolution -gt 0
  } |
  Select-Object -First 1
if (-not $activeGpu) {
  $activeGpu = $gpus | Where-Object { $_.Status -eq 'OK' } | Select-Object -First 1
}
$computer = Get-CimInstance Win32_ComputerSystem | Select-Object -First 1
$board = Get-CimInstance Win32_BaseBoard | Select-Object -First 1
$os = Get-CimInstance Win32_OperatingSystem | Select-Object -First 1
$instanceDrive = $null
try {
  if ($env:MINEBENCH_INSTANCE_DIR) {
    $instanceRoot = [System.IO.Path]::GetPathRoot(
      [System.IO.Path]::GetFullPath($env:MINEBENCH_INSTANCE_DIR))
    if ($instanceRoot -match '^[A-Za-z]:\\') {
      $instanceDrive = $instanceRoot.Substring(0, 2).ToUpperInvariant()
    }
  }
} catch {
  $instanceDrive = $null
}
$selectedDrive = $instanceDrive
$storageSelection = 'minecraft_instance_drive'
$disks = @(Get-CimInstance Win32_LogicalDisk)
$disk = $disks | Where-Object { $_.DeviceID -eq $selectedDrive } | Select-Object -First 1
if (-not $disk) {
  $selectedDrive = [string]$env:SystemDrive
  $storageSelection = 'system_drive_fallback'
  $disk = $disks | Where-Object { $_.DeviceID -eq $selectedDrive } | Select-Object -First 1
}
$physicalDisk = $null
try {
  if ($selectedDrive -match '^[A-Za-z]:$') {
    $partition = Get-Partition -DriveLetter $selectedDrive.Substring(0, 1) -ErrorAction Stop |
      Select-Object -First 1
    if ($partition) {
      $physicalDisk = Get-Disk -Number $partition.DiskNumber -ErrorAction Stop
    }
  }
} catch {
  $physicalDisk = $null
}
$gpuNames = ($gpus | ForEach-Object { $_.Name }) -join '; '
$gpuMemory = ($gpus | ForEach-Object { [uint64]$_.AdapterRAM } | Measure-Object -Sum).Sum
[ordered]@{
  cpu_name = [string]$cpu.Name
  cpu_threads = [string]$cpu.NumberOfLogicalProcessors
  cpu_physical_cores = [string]$cpu.NumberOfCores
  cpu_max_clock_mhz = [string]$cpu.MaxClockSpeed
  gpu_name = [string]$gpuNames
  gpu_memory_bytes = [string]$gpuMemory
  gpu_active_name = [string]$activeGpu.Name
  gpu_active_driver_version = [string]$activeGpu.DriverVersion
  gpu_active_selection = if ($activeGpu) { 'attached_display_or_first_ok' } else { 'unresolved' }
  ram_bytes = [string]$computer.TotalPhysicalMemory
  motherboard = ([string]$board.Manufacturer + ' ' + [string]$board.Product).Trim()
  storage_system_drive = [string]$env:SystemDrive
  storage_selected_drive = [string]$selectedDrive
  storage_selection = [string]$storageSelection
  storage_disk_model = [string]$physicalDisk.FriendlyName
  storage_media_type = [string]$physicalDisk.MediaType
  storage_bus_type = [string]$physicalDisk.BusType
  storage_total_bytes = [string]$disk.Size
  storage_free_bytes = [string]$disk.FreeSpace
  os_caption = [string]$os.Caption
  os_version = [string]$os.Version
  os_build = [string]$os.BuildNumber
  os_architecture = [string]$os.OSArchitecture
} | ConvertTo-Json -Compress
"#;

    let shell = find_powershell().context("PowerShell is not available for hardware collection")?;
    let mut command = Command::new(shell);
    command.env_remove("MINEBENCH_INSTANCE_DIR").args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    if let Some(instance_dir) = instance_dir {
        command.env("MINEBENCH_INSTANCE_DIR", instance_dir);
    }
    let output = command
        .output()
        .context("failed to invoke PowerShell for hardware collection")?;
    if !output.status.success() {
        bail!(
            "PowerShell hardware collection failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let json = decode_powershell_output(&output.stdout)?;
    let value: Value = serde_json::from_str(&json)
        .context("PowerShell hardware collection returned invalid JSON")?;
    let object = value
        .as_object()
        .context("PowerShell hardware collection returned a non-object")?;
    let mut values = BTreeMap::new();
    for (key, value) in object {
        if let Some(value) = value.as_str().filter(|value| !value.trim().is_empty()) {
            values.insert(key.clone(), value.to_string());
        } else if value.is_number() || value.is_boolean() {
            values.insert(key.clone(), value.to_string());
        }
    }
    if !values.contains_key("cpu_name") || !values.contains_key("os_caption") {
        bail!("PowerShell hardware collection returned incomplete CPU/OS data");
    }
    Ok(values)
}

fn decode_powershell_output(bytes: &[u8]) -> Result<String> {
    let text = if bytes.starts_with(&[0xff, 0xfe]) {
        decode_utf16(bytes, 2, true)?
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        decode_utf16(bytes, 2, false)?
    } else if looks_like_utf16(bytes, true) {
        decode_utf16(bytes, 0, true)?
    } else if looks_like_utf16(bytes, false) {
        decode_utf16(bytes, 0, false)?
    } else {
        String::from_utf8(bytes.to_vec()).context("PowerShell output is not UTF-8")?
    };
    Ok(text.trim_start_matches('\u{feff}').to_string())
}

fn decode_utf16(bytes: &[u8], offset: usize, little_endian: bool) -> Result<String> {
    let mut units = Vec::with_capacity((bytes.len() - offset) / 2);
    let (chunks, remainder) = bytes[offset..].as_chunks::<2>();
    for chunk in chunks {
        let unit = if little_endian {
            u16::from_le_bytes(*chunk)
        } else {
            u16::from_be_bytes(*chunk)
        };
        units.push(unit);
    }
    if !remainder.is_empty() {
        let encoding = if little_endian {
            "UTF-16LE"
        } else {
            "UTF-16BE"
        };
        bail!("PowerShell output has an incomplete {encoding} code unit");
    }
    let encoding = if little_endian {
        "UTF-16LE"
    } else {
        "UTF-16BE"
    };
    String::from_utf16(&units).with_context(|| format!("PowerShell output is invalid {encoding}"))
}

fn looks_like_utf16(bytes: &[u8], little_endian: bool) -> bool {
    let (pairs, _) = bytes.as_chunks::<2>();
    let pairs = &pairs[..pairs.len().min(128)];
    if pairs.len() < 2 {
        return false;
    }

    let zero_high_bytes = pairs
        .iter()
        .filter(|pair| {
            if little_endian {
                pair[1] == 0
            } else {
                pair[0] == 0
            }
        })
        .count();
    zero_high_bytes * 2 >= pairs.len()
}

fn find_powershell() -> Option<&'static str> {
    for name in ["powershell.exe", "pwsh.exe"] {
        if let Ok(result) = Command::new("where.exe").arg(name).output()
            && result.status.success()
            && !result.stdout.is_empty()
        {
            return Some(name);
        }
    }
    None
}

fn collect_fallback() -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    values.insert(
        "cpu_threads".to_string(),
        std::thread::available_parallelism()
            .map(|value| value.get().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
    );
    values.insert("collection_source".to_string(), "rust-fallback".to_string());
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_always_contains_cpu_thread_count() {
        assert!(collect_fallback().contains_key("cpu_threads"));
    }

    #[test]
    fn decodes_windows_powershell_utf16_output() {
        let mut bytes = vec![0xff, 0xfe];
        for unit in "{\"ok\":true}".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode_powershell_output(&bytes).unwrap(), "{\"ok\":true}");
    }

    #[test]
    fn decodes_bomless_utf16_output() {
        let input = "{\"ok\":true}";
        let little_endian = input
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let big_endian = input
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        assert_eq!(decode_powershell_output(&little_endian).unwrap(), input);
        assert_eq!(decode_powershell_output(&big_endian).unwrap(), input);
    }
}
