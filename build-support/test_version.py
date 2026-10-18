"""Exercise the shared build stamp through Cargo, including incremental rebuilds."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).with_name("version.rs").resolve()


class BuildVersionTests(unittest.TestCase):
    def test_build_provenance_and_cargo_invalidation(self):
        with tempfile.TemporaryDirectory(prefix="shock2-build-version-") as tmp:
            root = Path(tmp)
            crate = root / "runtimes/stamp-test"
            (crate / "src").mkdir(parents=True)
            (root / "build-support").mkdir()
            shutil.copyfile(SOURCE, root / "build-support/version.rs")
            (crate / "Cargo.toml").write_text(
                '[package]\nname="stamp-test"\nversion="0.1.0"\nedition="2021"\n'
            )
            (crate / "build.rs").write_text(
                '#[path = "../../build-support/version.rs"] mod version;\n'
                'fn main() { version::emit_build_label(); }\n'
            )
            (crate / "src/main.rs").write_text('''fn main() {
                println!("{}|{}|{}", env!("SHOCK2QUEST_BUILD_VERSION"),
                    env!("SHOCK2QUEST_BUILD_COMMIT"), env!("SHOCK2QUEST_BUILD_DIRTY"));
                println!("{}", env!("SHOCK2QUEST_BUILD_LABEL"));
            }''')
            # A tracked input outside this crate must also invalidate the stamp.
            (root / "asset.txt").write_text("original")
            (root / ".gitignore").write_text("target/\nCargo.lock\n")
            env = os.environ.copy()
            for key in ("SHOCK2QUEST_RELEASE_VERSION", "SHOCK2QUEST_GIT_SHA",
                        "SHOCK2QUEST_BUILD_CHANNEL", "CARGO_TARGET_DIR"):
                env.pop(key, None)

            def run(*args, overrides=None, cwd=None):
                return subprocess.check_output(
                    args, cwd=cwd or root, env=env | (overrides or {}), text=True,
                    stderr=subprocess.PIPE,
                ).strip()

            def stamp(**overrides):
                return run("cargo", "run", "--quiet", overrides=overrides, cwd=crate).split("\n")

            def build(**overrides):
                return stamp(**overrides)[0].split("|")

            def label(**overrides):
                return stamp(**overrides)[1]

            self.assertEqual(build(), ["dev", "unknown", "unknown"])
            run("git", "init", "-q")
            run("git", "config", "user.name", "Build Test")
            run("git", "config", "user.email", "build@example.invalid")
            run("git", "add", ".")
            run("git", "commit", "-qm", "initial")
            # Git was unavailable on the first build; start Git-aware tracking.
            run("cargo", "clean", "-q", cwd=crate)
            commit = run("git", "rev-parse", "HEAD")
            self.assertEqual(build(), ["dev", commit, "false"])
            output, = (crate / "target/debug/build").glob("stamp-test-*/output")
            modified = output.stat().st_mtime_ns
            self.assertEqual(build(), ["dev", commit, "false"])
            self.assertEqual(output.stat().st_mtime_ns, modified,
                             "unchanged builds must keep the cached build stamp")
            (root / "asset.txt").write_text("edited")
            self.assertEqual(build(), ["dev", commit, "true"])
            run("git", "add", "asset.txt")
            self.assertEqual(build(), ["dev", commit, "true"])
            run("git", "commit", "-qm", "update")
            commit = run("git", "rev-parse", "HEAD")
            self.assertEqual(build(), ["dev", commit, "false"])
            self.assertEqual(build(SHOCK2QUEST_RELEASE_VERSION="0.2.0",
                                   SHOCK2QUEST_GIT_SHA="a" * 40),
                             ["0.2.0", "a" * 40, "false"])
            self.assertEqual(build(), ["dev", commit, "false"])
            self.assertEqual(label(), f"dev | {commit[:7]}")
            # CI APKs name their channel; a release version overrides it.
            self.assertEqual(build(SHOCK2QUEST_BUILD_CHANNEL="pr"), ["pr", commit, "false"])
            self.assertEqual(label(SHOCK2QUEST_BUILD_CHANNEL="pr"), f"pr | {commit[:7]}")
            self.assertEqual(label(SHOCK2QUEST_BUILD_CHANNEL="pr",
                                   SHOCK2QUEST_RELEASE_VERSION="0.2.0",
                                   SHOCK2QUEST_GIT_SHA="a" * 40), "v0.2.0 | aaaaaaa")
            self.assertEqual(label(), f"dev | {commit[:7]}")
            # Source archives retain CI provenance even without a Git checkout.
            (root / ".git").rename(root / "archived-git")
            self.assertEqual(build(SHOCK2QUEST_RELEASE_VERSION="0.3.0",
                                   SHOCK2QUEST_GIT_SHA="b" * 40),
                             ["0.3.0", "b" * 40, "unknown"])


if __name__ == "__main__":
    unittest.main()
