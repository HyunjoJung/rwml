import pathlib
import unittest


WORKFLOW = (
    pathlib.Path(__file__).resolve().parents[1]
    / ".github"
    / "workflows"
    / "release.yml"
)


def step_body(text, name):
    marker = f"      - name: {name}\n"
    start = text.index(marker)
    end = text.find("\n      - ", start + len(marker))
    return text[start:] if end == -1 else text[start:end]


class ReleaseWorkflowTests(unittest.TestCase):
    def test_release_workflow_publishes_manifest_artifact(self):
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("python3 scripts/public_hygiene_audit.py", text)
        self.assertIn("python3 scripts/public_hygiene_audit.py --json", text)
        self.assertIn("cargo fmt --all -- --check", text)
        self.assertIn("cargo clippy --all-targets -- -D warnings", text)
        self.assertIn(
            "cargo clippy --all-targets --all-features -- -D warnings", text
        )
        self.assertIn("cargo test --all-targets --no-default-features", text)
        self.assertIn("cargo test --doc --all-features", text)
        self.assertIn("cargo doc --no-deps --all-features", text)
        self.assertIn(
            "python3 -m unittest discover -s tests -p 'test_*.py'", text
        )
        self.assertIn("scripts/release_manifest.py", text)
        self.assertIn("--git-rev \"$GITHUB_SHA\"", text)
        self.assertIn(
            'crate_version=$(cargo metadata --no-deps --format-version 1', text
        )
        self.assertIn('if [[ "$GITHUB_REF" == refs/tags/* ]]', text)
        self.assertIn('"$GITHUB_REF_NAME" != "v${crate_version}"', text)
        self.assertIn(
            'echo "RWML_VERSION=${crate_version}" >> "$GITHUB_ENV"', text
        )
        self.assertIn('--version "$RWML_VERSION"', text)
        self.assertNotIn('VERSION="${GITHUB_REF_NAME#v}"', text)
        self.assertIn("--release-policy public-release", text)
        self.assertNotIn("--enforce-policy-inputs", text)
        self.assertIn("--hygiene-report dist/public-hygiene.json", text)
        self.assertIn("--corpus-manifest corpus/public/MANIFEST.tsv", text)
        self.assertIn("--corpus-manifest corpus/public/RENDER_MANIFEST.tsv", text)
        self.assertIn("cargo test --all-targets --features render", text)
        self.assertIn(
            "cargo test --test bundled_fonts --all-features --locked", text
        )
        self.assertIn(
            "cargo test --release --test performance --locked -- --ignored --nocapture",
            text,
        )
        for command in [
            "cargo install wasm-bindgen-cli --version 0.2.126 --locked",
            "cargo build --lib --target wasm32-unknown-unknown --locked",
            "wasm-bindgen --target nodejs --out-dir target/wasm-node",
            "node tests/wasm_node_smoke.cjs target/wasm-node corpus/public/synthetic/comments.docx",
            "node tests/wasm_demo_report_format.mjs",
        ]:
            self.assertIn(command, text)
        self.assertIn(
            "cargo check --manifest-path fuzz/Cargo.toml --all-targets --locked",
            text,
        )
        self.assertIn("python3 scripts/gen_public_corpus.py --check", text)
        self.assertIn("dist/public-hygiene.json", text)
        self.assertIn("dist/rwml-release-manifest.json", text)
        self.assertIn("target/package/rwml-${RWML_VERSION}.crate", text)
        self.assertIn("target/package/rwml-${{ env.RWML_VERSION }}.crate", text)
        self.assertIn("actions/upload-artifact@v7", text)

    def test_release_workflow_checks_patch_compatible_public_api(self):
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("fetch-depth: 0", text)
        self.assertIn(
            "cargo install cargo-semver-checks --version 0.48.0 --locked", text
        )
        self.assertIn(
            "cargo semver-checks check-release --baseline-rev v0.1.0 "
            "--release-type patch --default-features",
            text,
        )

    def test_release_workflow_publishes_font_dependency_before_main_package(self):
        text = WORKFLOW.read_text(encoding="utf-8")

        font_package = "cargo package --manifest-path rwml-fonts/Cargo.toml"
        font_publish_step = "- name: Publish bundled font crate dependency"
        main_package_step = "- name: Package main crate"
        font_publish = step_body(text, "Publish bundled font crate dependency")
        main_package = step_body(text, "Package main crate")
        main_identity = step_body(text, "Verify main crate registry identity")
        manifest = step_body(text, "Generate release manifest")
        upload = step_body(text, "Upload release manifest artifacts")
        main_publish = step_body(text, "Publish main crate")
        self.assertIn("cargo test --manifest-path rwml-fonts/Cargo.toml --locked", text)
        self.assertIn(font_package, text)
        self.assertIn("cargo package --list > /dev/null", text)
        self.assertIn(font_publish_step, text)
        self.assertIn('if [[ "$font_version" != "$RWML_VERSION" ]]', font_publish)
        self.assertIn("CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}", font_publish)
        self.assertIn("python3 scripts/publish_crate.py", font_publish)
        self.assertIn("--name rwml-fonts", font_publish)
        self.assertIn("--manifest-path rwml-fonts/Cargo.toml", font_publish)
        self.assertIn(
            'rwml-fonts/target/package/rwml-fonts-${font_version}.crate',
            font_publish,
        )
        self.assertIn(main_package_step, text)
        self.assertIn("for attempt in {1..12}", main_package)
        self.assertIn('cargo info "rwml-fonts@${RWML_VERSION}"', main_package)
        self.assertIn("registry_visible=true", main_package)
        package_commands = [
            line.strip() for line in main_package.splitlines() if line.strip() == "cargo package"
        ]
        self.assertEqual(package_commands, ["cargo package"])
        self.assertLess(main_package.index("cargo info"), main_package.index("cargo package"))
        self.assertIn("python3 scripts/publish_crate.py", main_identity)
        self.assertIn("--name rwml", main_identity)
        self.assertIn('target/package/rwml-${RWML_VERSION}.crate', main_identity)
        self.assertIn("--check-only", main_identity)
        self.assertIn("target/package/rwml-${RWML_VERSION}.crate", manifest)
        self.assertIn(
            "rwml-fonts/target/package/rwml-fonts-${RWML_VERSION}.crate", manifest
        )
        self.assertIn("target/package/rwml-${{ env.RWML_VERSION }}.crate", upload)
        self.assertIn(
            "rwml-fonts/target/package/rwml-fonts-${{ env.RWML_VERSION }}.crate",
            upload,
        )
        self.assertIn("CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}", main_publish)
        self.assertIn("python3 scripts/publish_crate.py", main_publish)
        self.assertIn("--name rwml", main_publish)
        self.assertIn('target/package/rwml-${RWML_VERSION}.crate', main_publish)
        self.assertNotIn("--token", text)
        self.assertLess(text.index(font_package), text.index(font_publish_step))
        self.assertLess(text.index(font_publish_step), text.index(main_package_step))
        ordered_steps = [
            "- name: Package main crate",
            "- name: Verify main crate registry identity",
            "- name: Generate release manifest",
            "- name: Upload release manifest artifacts",
            "- name: Publish main crate",
        ]
        positions = [text.index(step) for step in ordered_steps]
        self.assertEqual(positions, sorted(positions))
        self.assertIn(
            "cargo semver-checks check-release --baseline-rev v0.1.0 "
            "--release-type patch --all-features",
            text,
        )


if __name__ == "__main__":
    unittest.main()
