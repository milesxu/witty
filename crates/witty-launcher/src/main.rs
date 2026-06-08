fn main() -> anyhow::Result<()> {
    witty_launcher::run_cli(std::env::args().skip(1))
}
