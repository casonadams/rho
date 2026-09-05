use super::super::*;
use std::path::Path;

#[tokio::test]
async fn test_dynamic_path_activation_in_monorepo() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_dyn_monorepo_{}", uuid::Uuid::new_v4()));
    let repo_root = temp_dir.join("repo");
    let plugin_crate = repo_root.join("crates").join("rho-plugin-sdk");
    let plugin_src = plugin_crate.join("src");

    tokio::fs::create_dir_all(repo_root.join(".git")).await.unwrap();
    tokio::fs::create_dir_all(&plugin_src).await.unwrap();

    tokio::fs::write(repo_root.join("AGENTS.md"), "# Root Instructions\n")
        .await
        .unwrap();
    tokio::fs::write(plugin_crate.join("AGENTS.md"), "# Plugin SDK Instructions\n")
        .await
        .unwrap();
    tokio::fs::write(plugin_src.join("lib.rs"), "pub fn run() {}\n")
        .await
        .unwrap();

    let mut ctx = ProjectContext::discover(&repo_root, None).await;
    assert_eq!(ctx.instruction_files.len(), 1);
    assert_eq!(ctx.instruction_files[0].1, "# Root Instructions");

    ctx.activate_path_instructions(Path::new("crates/rho-plugin-sdk/src/lib.rs"));

    assert_eq!(ctx.instruction_files.len(), 2);
    assert_eq!(ctx.instruction_files[0].1, "# Root Instructions");
    assert_eq!(ctx.instruction_files[1].1, "# Plugin SDK Instructions");

    let prompt = ctx.build_system_prompt();
    let root_idx = prompt.find("# Root Instructions").unwrap();
    let plugin_idx = prompt.find("# Plugin SDK Instructions").unwrap();
    assert!(root_idx < plugin_idx);

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_dynamic_path_activation_deduplication() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_dyn_dedup_{}", uuid::Uuid::new_v4()));
    let repo_root = temp_dir.join("repo");
    let sub = repo_root.join("packages").join("pkg-a");

    tokio::fs::create_dir_all(repo_root.join(".git")).await.unwrap();
    tokio::fs::create_dir_all(&sub).await.unwrap();

    tokio::fs::write(repo_root.join("AGENTS.md"), "# Root\n").await.unwrap();
    tokio::fs::write(sub.join("AGENTS.md"), "# Package A\n").await.unwrap();
    tokio::fs::write(sub.join("a.rs"), "fn a() {}\n").await.unwrap();
    tokio::fs::write(sub.join("b.rs"), "fn b() {}\n").await.unwrap();

    let mut ctx = ProjectContext::discover(&repo_root, None).await;
    assert_eq!(ctx.instruction_files.len(), 1);

    ctx.activate_path_instructions(Path::new("packages/pkg-a/a.rs"));
    assert_eq!(ctx.instruction_files.len(), 2);

    ctx.activate_path_instructions(Path::new("packages/pkg-a/b.rs"));
    assert_eq!(ctx.instruction_files.len(), 2);

    ctx.activate_path_instructions(Path::new("packages/pkg-a/a.rs"));
    assert_eq!(ctx.instruction_files.len(), 2);

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_dynamic_path_activation_bounds_file_limit() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_dyn_files_bound_{}", uuid::Uuid::new_v4()));
    let repo_root = temp_dir.join("repo");
    tokio::fs::create_dir_all(repo_root.join(".git")).await.unwrap();

    for i in 0..15 {
        let pkg_dir = repo_root.join("packages").join(format!("pkg_{i}"));
        tokio::fs::create_dir_all(&pkg_dir).await.unwrap();
        tokio::fs::write(pkg_dir.join("AGENTS.md"), format!("# Pkg {i}\n"))
            .await
            .unwrap();
        tokio::fs::write(pkg_dir.join("main.rs"), "fn main() {}\n")
            .await
            .unwrap();
    }

    let mut ctx = ProjectContext::discover(&repo_root, None).await;
    assert_eq!(ctx.instruction_files.len(), 0);

    for i in 0..15 {
        ctx.activate_path_instructions(Path::new(&format!("packages/pkg_{i}/main.rs")));
    }

    assert_eq!(ctx.dynamic_instructions_count, MAX_DYNAMIC_INSTRUCTION_FILES);
    assert_eq!(ctx.instruction_files.len(), MAX_DYNAMIC_INSTRUCTION_FILES);

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_dynamic_path_activation_bounds_byte_limit() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_dyn_bytes_bound_{}", uuid::Uuid::new_v4()));
    let repo_root = temp_dir.join("repo");
    let sub = repo_root.join("sub");

    tokio::fs::create_dir_all(repo_root.join(".git")).await.unwrap();
    tokio::fs::create_dir_all(&sub).await.unwrap();

    let large_instructions = "x".repeat(70 * 1024);
    tokio::fs::write(sub.join("AGENTS.md"), &large_instructions)
        .await
        .unwrap();
    tokio::fs::write(sub.join("file.rs"), "fn foo() {}\n").await.unwrap();

    let mut ctx = ProjectContext::discover(&repo_root, None).await;
    ctx.activate_path_instructions(Path::new("sub/file.rs"));

    assert_eq!(ctx.dynamic_instructions_bytes, MAX_DYNAMIC_INSTRUCTION_BYTES);
    assert_eq!(ctx.instruction_files.len(), 1);
    assert_eq!(ctx.instruction_files[0].1.len(), MAX_DYNAMIC_INSTRUCTION_BYTES);

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_dynamic_path_activation_respects_no_context_files() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_dyn_no_ctx_{}", uuid::Uuid::new_v4()));
    let repo_root = temp_dir.join("repo");
    let sub = repo_root.join("sub");

    tokio::fs::create_dir_all(repo_root.join(".git")).await.unwrap();
    tokio::fs::create_dir_all(&sub).await.unwrap();

    tokio::fs::write(repo_root.join("AGENTS.md"), "# Root\n").await.unwrap();
    tokio::fs::write(sub.join("AGENTS.md"), "# Sub\n").await.unwrap();
    tokio::fs::write(sub.join("main.rs"), "fn main() {}\n").await.unwrap();

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

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_dynamic_path_activation_confinement() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_dyn_confine_{}", uuid::Uuid::new_v4()));
    let repo_root = temp_dir.join("repo");
    let external_dir = temp_dir.join("external");

    tokio::fs::create_dir_all(repo_root.join(".git")).await.unwrap();
    tokio::fs::create_dir_all(&external_dir).await.unwrap();

    tokio::fs::write(repo_root.join("AGENTS.md"), "# Repo Rules\n")
        .await
        .unwrap();
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

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_dynamic_path_activation_with_transclusion() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_dyn_trans_{}", uuid::Uuid::new_v4()));
    let repo_root = temp_dir.join("repo");
    let sub = repo_root.join("crates").join("sub");
    let docs = sub.join("docs");

    tokio::fs::create_dir_all(repo_root.join(".git")).await.unwrap();
    tokio::fs::create_dir_all(&docs).await.unwrap();

    tokio::fs::write(docs.join("standards.md"), "Subtree coding standards.\n")
        .await
        .unwrap();
    tokio::fs::write(sub.join("AGENTS.md"), "# Subtree Rules\n@docs/standards.md\n")
        .await
        .unwrap();
    tokio::fs::write(sub.join("mod.rs"), "pub mod sub;\n").await.unwrap();

    let mut ctx = ProjectContext::discover(&repo_root, None).await;
    assert_eq!(ctx.instruction_files.len(), 0);

    ctx.activate_path_instructions(Path::new("crates/sub/mod.rs"));
    assert_eq!(ctx.instruction_files.len(), 1);
    assert!(ctx.instruction_files[0].1.contains("# Subtree Rules"));
    assert!(ctx.instruction_files[0].1.contains("Subtree coding standards."));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}
