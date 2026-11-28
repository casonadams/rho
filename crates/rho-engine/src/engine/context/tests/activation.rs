use super::super::activation::activate_path_instructions_async;
use super::super::*;
use crate::test_support::TestDir;
use std::path::{Path, PathBuf};

const MONOREPO: &[(&str, &str)] = &[
    ("AGENTS.md", "# Root Instructions\n"),
    ("crates/rho-subcrate/AGENTS.md", "# Subcrate Instructions\n"),
    ("crates/rho-subcrate/src/lib.rs", "pub fn run() {}\n"),
];

async fn fixture_repo(files: &[(impl AsRef<str>, impl AsRef<str>)]) -> (TestDir, PathBuf) {
    let temp_dir = TestDir::new();
    let repo_root = temp_dir.join("repo");
    tokio::fs::create_dir_all(repo_root.join(".git")).await.unwrap();
    for (rel, text) in files {
        let path = repo_root.join(rel.as_ref());
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        tokio::fs::write(path, text.as_ref()).await.unwrap();
    }
    (temp_dir, repo_root)
}

#[tokio::test]
async fn test_dynamic_path_activation_in_monorepo() {
    let (_temp_dir, repo_root) = fixture_repo(MONOREPO).await;

    let mut ctx = ProjectContext::discover(&repo_root, None).await;
    assert_eq!(
        (ctx.instruction_files.len(), ctx.instruction_files[0].1.as_str()),
        (1, "# Root Instructions")
    );

    ctx.activate_path_instructions(Path::new("crates/rho-subcrate/src/lib.rs"));
    assert_eq!(ctx.instruction_files.len(), 2);
    assert_eq!(ctx.instruction_files[1].1, "# Subcrate Instructions");

    let prompt = ctx.build_system_prompt();
    let root_idx = prompt.find("# Root Instructions").unwrap();
    let sub_idx = prompt.find("# Subcrate Instructions").unwrap();
    assert!(root_idx < sub_idx);
}

#[tokio::test]
async fn dynamic_instructions_are_cut_to_the_byte_budget_on_a_character_boundary() {
    let (_temp_dir, repo_root) = fixture_repo(&[("pkg/AGENTS.md", "éé"), ("pkg/a.rs", "fn a() {}\n")]).await;

    let mut ctx = ProjectContext::discover(&repo_root, None).await;
    ctx.dynamic_instructions_bytes = super::super::activation::MAX_DYNAMIC_INSTRUCTION_BYTES - 3;
    ctx.activate_path_instructions(Path::new("pkg/a.rs"));

    assert_eq!(ctx.instruction_files.last().unwrap().1, "é");
}

#[tokio::test]
async fn test_dynamic_path_activation_deduplication() {
    let (_temp_dir, repo_root) = fixture_repo(&[
        ("AGENTS.md", "# Root\n"),
        ("packages/pkg-a/AGENTS.md", "# Package A\n"),
        ("packages/pkg-a/a.rs", "fn a() {}\n"),
        ("packages/pkg-a/b.rs", "fn b() {}\n"),
    ])
    .await;

    let mut ctx = ProjectContext::discover(&repo_root, None).await;
    assert_eq!(ctx.instruction_files.len(), 1);

    ctx.activate_path_instructions(Path::new("packages/pkg-a/a.rs"));
    assert_eq!(ctx.instruction_files.len(), 2);

    ctx.activate_path_instructions(Path::new("packages/pkg-a/b.rs"));
    assert_eq!(ctx.instruction_files.len(), 2);

    ctx.activate_path_instructions(Path::new("packages/pkg-a/a.rs"));
    assert_eq!(ctx.instruction_files.len(), 2);
}

#[tokio::test]
async fn test_dynamic_path_activation_bounds_file_limit() {
    let files: Vec<(String, String)> = (0..15)
        .flat_map(|i| {
            [
                (format!("packages/pkg_{i}/AGENTS.md"), format!("# Pkg {i}\n")),
                (format!("packages/pkg_{i}/main.rs"), "fn main() {}\n".to_string()),
            ]
        })
        .collect();
    let (_temp_dir, repo_root) = fixture_repo(&files).await;

    let mut ctx = ProjectContext::discover(&repo_root, None).await;
    assert_eq!(ctx.instruction_files.len(), 0);

    for i in 0..15 {
        ctx.activate_path_instructions(Path::new(&format!("packages/pkg_{i}/main.rs")));
    }

    assert_eq!(ctx.dynamic_instructions_count, MAX_DYNAMIC_INSTRUCTION_FILES);
    assert_eq!(ctx.instruction_files.len(), MAX_DYNAMIC_INSTRUCTION_FILES);
}

#[tokio::test]
async fn test_dynamic_path_activation_bounds_byte_limit() {
    let large_instructions = "x".repeat(70 * 1024);
    let (_temp_dir, repo_root) = fixture_repo(&[
        ("sub/AGENTS.md", large_instructions.as_str()),
        ("sub/file.rs", "fn foo() {}\n"),
    ])
    .await;

    let mut ctx = ProjectContext::discover(&repo_root, None).await;
    ctx.activate_path_instructions(Path::new("sub/file.rs"));

    assert_eq!(ctx.dynamic_instructions_bytes, MAX_DYNAMIC_INSTRUCTION_BYTES);
    assert_eq!(ctx.instruction_files.len(), 1);
    assert_eq!(ctx.instruction_files[0].1.len(), MAX_DYNAMIC_INSTRUCTION_BYTES);
}

#[tokio::test]
async fn test_dynamic_path_activation_respects_no_context_files() {
    let (_temp_dir, repo_root) = fixture_repo(&[
        ("AGENTS.md", "# Root\n"),
        ("sub/AGENTS.md", "# Sub\n"),
        ("sub/main.rs", "fn main() {}\n"),
    ])
    .await;

    let mut ctx = ProjectContext::discover_with_dirs(
        &repo_root,
        ContextDirs {
            no_context_files: true,
            ..Default::default()
        },
    )
    .await;

    assert!(ctx.instruction_files.is_empty());
    ctx.activate_path_instructions(Path::new("sub/main.rs"));
    assert!(ctx.instruction_files.is_empty());
}

#[tokio::test]
async fn test_dynamic_path_activation_confinement() {
    let (temp_dir, repo_root) = fixture_repo(&[("AGENTS.md", "# Repo Rules\n")]).await;
    let external_dir = temp_dir.join("external");
    tokio::fs::create_dir_all(&external_dir).await.unwrap();
    tokio::fs::write(external_dir.join("AGENTS.md"), "# External Secret Rules\n")
        .await
        .unwrap();
    tokio::fs::write(external_dir.join("ext.rs"), "fn ext() {}\n")
        .await
        .unwrap();

    let mut ctx = ProjectContext::discover(&repo_root, None).await;
    assert_eq!(ctx.instruction_files.len(), 1);

    ctx.activate_path_instructions(&external_dir.join("ext.rs"));
    assert_eq!(ctx.instruction_files.len(), 1);
    assert_eq!(ctx.instruction_files[0].1, "# Repo Rules");
}

#[tokio::test]
async fn test_dynamic_path_activation_with_transclusion() {
    let (_temp_dir, repo_root) = fixture_repo(&[
        ("crates/sub/docs/standards.md", "Subtree coding standards.\n"),
        ("crates/sub/AGENTS.md", "# Subtree Rules\n@docs/standards.md\n"),
        ("crates/sub/mod.rs", "pub mod sub;\n"),
    ])
    .await;

    let mut ctx = ProjectContext::discover(&repo_root, None).await;
    assert_eq!(ctx.instruction_files.len(), 0);

    ctx.activate_path_instructions(Path::new("crates/sub/mod.rs"));
    assert_eq!(ctx.instruction_files.len(), 1);
    assert!(ctx.instruction_files[0].1.contains("# Subtree Rules"));
    assert!(ctx.instruction_files[0].1.contains("Subtree coding standards."));
}

#[tokio::test]
async fn test_async_path_activation_loads_subcrate_and_honors_no_context() {
    let (_temp_dir, repo_root) = fixture_repo(MONOREPO).await;

    let mut ctx = ProjectContext::discover(&repo_root, None).await;
    activate_path_instructions_async(&mut ctx, Path::new("crates/rho-subcrate/src/lib.rs")).await;
    assert_eq!(ctx.instruction_files.len(), 2);
    assert_eq!(ctx.instruction_files[1].1, "# Subcrate Instructions");

    let mut disabled = ProjectContext::discover_with_dirs(
        &repo_root,
        ContextDirs {
            no_context_files: true,
            ..Default::default()
        },
    )
    .await;
    activate_path_instructions_async(&mut disabled, Path::new("crates/rho-subcrate/src/lib.rs")).await;
    assert!(disabled.instruction_files.is_empty());
}
