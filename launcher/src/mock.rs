#[allow(dead_code)]
mod implementation {
    use crate as minebench;
    include!("bin/mock-benchmark.rs");
}

pub fn run_from<I>(arguments: I) -> anyhow::Result<()>
where
    I: IntoIterator<Item = String>,
{
    implementation::run_from(arguments)
}
