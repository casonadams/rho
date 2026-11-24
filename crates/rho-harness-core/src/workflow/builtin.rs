/// Workflow bundles compiled into the binary: relative path and contents of every file in the bundle.
pub(super) const BUILTINS: &[(&str, &[(&str, &str)])] = &[("audit", AUDIT)];

macro_rules! bundle_file {
    ($bundle:literal, $path:literal) => {
        ($path, include_str!(concat!("builtin/", $bundle, "/", $path)))
    };
}

const AUDIT: &[(&str, &str)] = &[
    bundle_file!("audit", "workflow.xml"),
    bundle_file!("audit", "agents/curator.md"),
    bundle_file!("audit", "agents/hunter-architecture.md"),
    bundle_file!("audit", "agents/hunter-compatibility.md"),
    bundle_file!("audit", "agents/hunter-correctness.md"),
    bundle_file!("audit", "agents/hunter-maintainability.md"),
    bundle_file!("audit", "agents/hunter-operability.md"),
    bundle_file!("audit", "agents/hunter-performance.md"),
    bundle_file!("audit", "agents/hunter-reliability.md"),
    bundle_file!("audit", "agents/hunter-security.md"),
    bundle_file!("audit", "agents/hunter-testing.md"),
    bundle_file!("audit", "agents/hunter-usability.md"),
    bundle_file!("audit", "agents/recon.md"),
    bundle_file!("audit", "agents/skeptic.md"),
    bundle_file!("audit", "agents/tracer.md"),
    bundle_file!("audit", "prompts/check.md"),
    bundle_file!("audit", "prompts/check-repro.md"),
    bundle_file!("audit", "prompts/dedupe.md"),
    bundle_file!("audit", "prompts/digest.md"),
    bundle_file!("audit", "prompts/gaps.md"),
    bundle_file!("audit", "prompts/hunt.md"),
    bundle_file!("audit", "prompts/recon.md"),
    bundle_file!("audit", "prompts/report.md"),
    bundle_file!("audit", "prompts/trace.md"),
];
