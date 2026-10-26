mod adapter;
mod clock;

fn main() -> anyhow::Result<()> {
    betteryam_runtime::run(adapter::YandexMini3)
}
