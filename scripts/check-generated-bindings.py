"""Regenerate the checked-in TypeScript DTO destination and fail on drift."""

from pathlib import Path
import shutil
import subprocess
import os


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "crates" / "manager-app" / "bindings"
DESTINATION = ROOT / "apps" / "desktop" / "src" / "shared" / "api" / "generated"


def main() -> None:
    DESTINATION.mkdir(parents=True, exist_ok=True)

    generated_names = {path.name for path in SOURCE.glob("*.ts")}
    for path in DESTINATION.glob("*.ts"):
        if path.name != "index.ts" and path.name not in generated_names:
            path.unlink()

    for source in SOURCE.glob("*.ts"):
        shutil.copyfile(source, DESTINATION / source.name)

    subprocess.run(
        [
            "pnpm.cmd" if os.name == "nt" else "pnpm",
            "--dir",
            "apps/desktop",
            "exec",
            "biome",
            "format",
            "--write",
            "src/shared/api/generated",
        ],
        cwd=ROOT,
        check=True,
    )

    changed = subprocess.run(
        ["git", "diff", "--exit-code", "--", str(DESTINATION.relative_to(ROOT))],
        cwd=ROOT,
        check=False,
    )
    untracked = subprocess.run(
        [
            "git",
            "ls-files",
            "--others",
            "--exclude-standard",
            "--",
            str(DESTINATION.relative_to(ROOT)),
        ],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if changed.returncode or untracked:
        raise SystemExit(
            "Generated DTOs are out of date. Run `cargo test -p manager-app`, "
            "then `python scripts/check-generated-bindings.py` and commit the result."
        )


if __name__ == "__main__":
    main()
