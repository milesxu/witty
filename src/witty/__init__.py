"""Witty Terminal - AI-powered terminal emulator."""

__version__ = "0.1.0"


def main() -> None:
    """Entry point for the witty command."""
    from witty.gui import run_gui

    run_gui()


if __name__ == "__main__":
    main()