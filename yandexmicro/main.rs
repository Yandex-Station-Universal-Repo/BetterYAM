mod adapter;

fn main() -> anyhow::Result<()> {
    betteryam_runtime::run(adapter::YandexMicro)
}
