"""Expose the repository toolchain as a GitHub Actions step output."""

from pathlib import Path
import tomllib


def main() -> None:
    source = Path(__file__).resolve().parents[1] / "rust-toolchain.toml"
    with source.open("rb") as file:
        channel = tomllib.load(file)["toolchain"]["channel"]
    if not isinstance(channel, str) or not channel or "\n" in channel or "\r" in channel:
        raise SystemExit("rust-toolchain.toml must contain a single toolchain channel")
    print(f"toolchain={channel}")


if __name__ == "__main__":
    main()
