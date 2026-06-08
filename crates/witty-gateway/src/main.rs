fn main() -> anyhow::Result<()> {
    witty_gateway::run_cli(std::env::args().skip(1))
}
