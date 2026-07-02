//! Object-level witnesses for the production git boundary.
//!
//! The bump commit is built as blob/tree/commit objects with no working
//! copy, index, or branch involvement: the witness repository never gets a
//! checkout, and no ref moves.

use gix::objs::tree::EntryKind;

use synchronizer::configuration::{BranchScheme, CommitAuthor};
use synchronizer::git_repository::{
    CommitMessage, ComponentRepository, FileEdit, GitRepository, RepositoryFilePath,
};
use synchronizer::types::{
    AuthorEmail, AuthorName, BranchName, CommitIdentifier, ComponentName, RepositoryUrl,
};

/// A bare fixture repository with one root commit carrying two files.
fn fixture_repository(directory: &std::path::Path) -> CommitIdentifier {
    let repository = gix::init_bare(directory).expect("bare fixture repository initializes");
    let manifest = repository
        .write_blob(b"[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n")
        .expect("blob writes");
    let readme = repository
        .write_blob(b"fixture readme\n")
        .expect("blob writes");
    let empty_tree = repository.empty_tree();
    let mut editor = repository
        .edit_tree(empty_tree.id())
        .expect("tree editor opens");
    editor
        .upsert("Cargo.toml", EntryKind::Blob, manifest.detach())
        .expect("tree entry upserts");
    editor
        .upsert("README.md", EntryKind::Blob, readme.detach())
        .expect("tree entry upserts");
    let tree = editor.write().expect("tree writes");
    let signature = gix::actor::Signature {
        name: "fixture".into(),
        email: "fixture@example.net".into(),
        time: gix::date::Time::new(1_750_000_000, 0),
    };
    let commit = gix::objs::Commit {
        tree: tree.detach(),
        parents: Default::default(),
        author: signature.clone(),
        committer: signature,
        encoding: None,
        message: "fixture root".into(),
        extra_headers: Vec::new(),
    };
    let commit_id = repository.write_object(&commit).expect("commit writes");
    CommitIdentifier::new(commit_id.detach().to_string())
}

#[test]
fn commits_are_built_at_the_object_level_without_touching_any_ref() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let base = fixture_repository(scratch.path());
    let repository = GitRepository::open(
        ComponentName::new("fixture"),
        scratch.path().to_path_buf(),
        RepositoryUrl::new("https://github.com/LiGoldragon/fixture.git"),
        BranchScheme::new(BranchName::new("main"), BranchName::new("synchronizer")),
        CommitAuthor::new(
            AuthorName::new("release-bot"),
            AuthorEmail::new("release@example.org"),
        ),
    )
    .expect("the boundary opens the clone");

    // Reading at a revision goes through the object store.
    let manifest = repository
        .file_at(&base, &RepositoryFilePath::cargo_manifest())
        .expect("object read succeeds")
        .expect("the file exists at the revision");
    assert!(manifest.contains("name = \"fixture\""));
    assert_eq!(
        repository
            .file_at(&base, &RepositoryFilePath::flake_lock())
            .expect("object read succeeds"),
        None,
        "an absent file reads as None, not an error"
    );

    // The bump commit: one file replaced, one file inherited, parent = base.
    let edited = "[package]\nname = \"fixture\"\nversion = \"0.2.0\"\n";
    let tip = repository
        .commit_file_edits(
            &base,
            &[FileEdit::new(
                RepositoryFilePath::cargo_manifest(),
                edited.to_string(),
            )],
            &CommitMessage::new("synchronizer: fixture bump"),
        )
        .expect("the object-level commit builds");
    let bumped = repository
        .file_at(&tip, &RepositoryFilePath::cargo_manifest())
        .expect("object read succeeds")
        .expect("the edited file exists at the new revision");
    assert_eq!(bumped, edited);
    let inherited = repository
        .file_at(&tip, &RepositoryFilePath::new("README.md"))
        .expect("object read succeeds")
        .expect("untouched files carry over from the base tree");
    assert_eq!(inherited, "fixture readme\n");

    // The commit carries the configured author, not any tool-baked identity.
    let reopened = gix::open(scratch.path()).expect("fixture reopens");
    let committed = reopened
        .find_object(gix::ObjectId::from_hex(tip.as_str().as_bytes()).expect("tip is an object id"))
        .expect("the bump commit is in the object store")
        .try_into_commit()
        .expect("the object is a commit");
    let author = committed.author().expect("the commit has an author");
    assert_eq!(author.name, "release-bot");
    assert_eq!(author.email, "release@example.org");

    // No ref anywhere moved: the commit exists only in the object store.
    let references: Vec<_> = reopened
        .references()
        .expect("reference platform opens")
        .all()
        .expect("reference iteration starts")
        .filter_map(Result::ok)
        .map(|reference| reference.name().as_bstr().to_string())
        .collect();
    assert!(
        references.is_empty(),
        "object-level commits must move no ref, found: {references:?}"
    );

    // The full tree materializes for the transitive-lock fallback.
    let files = repository
        .tree_files_at(&tip)
        .expect("tree files read from the object store");
    let mut paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
    paths.sort_unstable();
    assert_eq!(paths, vec!["Cargo.toml", "README.md"]);
}
