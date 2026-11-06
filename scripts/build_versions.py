"""Build a pinned Minecraft version, verify Mixin targets, and collect installable JARs."""
from pathlib import Path
import argparse, json, os, shutil, subprocess, sys, zipfile

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--version', help='Build only this Minecraft version; default: all supported versions')
args = parser.parse_args()
matrix = json.loads((root/'minecraft-versions.json').read_text(encoding='utf-8'))
if args.version and args.version not in [x['minecraft'] for x in matrix]:
    parser.error('Unsupported version: '+args.version)
for target in matrix:
    version = target['minecraft']
    if args.version and version != args.version:
        continue
    project = root/target['directory']
    wrapper = project/('gradlew.bat' if os.name == 'nt' else 'gradlew')
    command = [str(wrapper)] if os.name == 'nt' else ['sh', str(wrapper)]
    command += ['-p', str(project), 'build', '--console', 'plain',
                '-Dfabric.loom.ci=true',
                '-Dorg.gradle.internal.http.connectionTimeout=30000',
                '-Dorg.gradle.internal.http.socketTimeout=30000']
    command += ['-P'+key+'='+value for key, value in target['properties'].items()]
    print('Building Minecraft '+version, flush=True)
    subprocess.run(command, check=True)
    subprocess.run([sys.executable, str(root/'scripts/verify_mixins.py'), str(project), version], check=True)
    destination = root/'build/versions'/version
    destination.mkdir(parents=True, exist_ok=True)
    matched = []
    for jar in (project/'build/libs').glob('*.jar'):
        if '-sources' in jar.name or '-dev' in jar.name:
            continue
        with zipfile.ZipFile(jar) as archive:
            metadata = json.loads(archive.read('fabric.mod.json'))
            assert metadata['id'] == 'mcbenchmark', 'Unexpected Adapter mod id'
            if metadata['depends']['minecraft'] != version:
                continue
            assert 'fabric-api' in metadata['depends'], 'Missing Fabric API requirement'
            assert 'fabric' not in metadata['depends'], 'Obsolete Fabric API dependency id'
            minimum_java = 25 if version.startswith('26.') else 21
            for path in archive.namelist():
                if path.endswith('.class'):
                    major = int.from_bytes(archive.read(path)[6:8], 'big')
                    assert major <= minimum_java + 44, 'Unexpected Java bytecode version: '+path
            for entries in metadata.get('entrypoints', {}).values():
                for entry in entries:
                    name = entry if isinstance(entry, str) else entry['value']
                    assert name.replace('.', '/')+'.class' in archive.namelist(), 'Missing entrypoint '+name
            for mixin in metadata.get('mixins', []):
                config = mixin if isinstance(mixin, str) else mixin['config']
                definition = json.loads(archive.read(config))
                for entry in definition.get('client', [])+definition.get('mixins', []):
                    path = (definition['package']+'.'+entry).replace('.', '/')+'.class'
                    assert path in archive.namelist(), 'Missing Mixin class '+path
        shutil.copy2(jar, destination/jar.name)
        matched.append(jar.name)
    if not matched:
        raise RuntimeError('No installable JAR for '+version)
    print('Verified JARs: '+', '.join(matched), flush=True)
