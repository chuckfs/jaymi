//! Integration tests for Layer 4 Slice 4 — Personal Memory.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi::Application;
use jaymi_core::UserRequest;
use jaymi_memory::{
    CreatePersonalMemoryRequest, MemoryEngine, MemoryEngineApi, PersonalMemoryKind,
    UpdatePersonalMemoryRequest,
};
use jaymi_planner::Planner;

#[test]
fn personal_preferences_persist_update_and_are_retrieved_intentionally() {
    let data_dir = temp_dir("personal-memory");
    let app = Application::boot_with_data_dir(&data_dir).expect("boot");

    let name = app
        .create_personal_memory(&CreatePersonalMemoryRequest {
            kind: PersonalMemoryKind::PreferredName,
            summary: "Preferred name".into(),
            content: "Charlie".into(),
            importance: Some(90),
            confidence: Some(95),
            tags: vec![],
            source: Some("user_request".into()),
        })
        .expect("create name");

    let writing = app
        .create_personal_memory(&CreatePersonalMemoryRequest {
            kind: PersonalMemoryKind::WritingStyle,
            summary: "Writing style".into(),
            content: "Concise, technical, no filler.".into(),
            importance: Some(80),
            confidence: Some(85),
            tags: vec![],
            source: Some("user_request".into()),
        })
        .expect("create writing");

    let code = app
        .create_personal_memory(&CreatePersonalMemoryRequest {
            kind: PersonalMemoryKind::CodeStyle,
            summary: "Code style".into(),
            content: "Prefer explicit Rust, small crates, no drive-by refactors.".into(),
            importance: Some(85),
            confidence: Some(90),
            tags: vec![],
            source: Some("user_request".into()),
        })
        .expect("create code");

    let editor = app
        .create_personal_memory(&CreatePersonalMemoryRequest {
            kind: PersonalMemoryKind::FavoriteEditor,
            summary: "Favorite editor".into(),
            content: "Cursor".into(),
            importance: Some(70),
            confidence: Some(90),
            tags: vec![],
            source: Some("user_request".into()),
        })
        .expect("create editor");

    let theme = app
        .create_personal_memory(&CreatePersonalMemoryRequest {
            kind: PersonalMemoryKind::PreferredTheme,
            summary: "Preferred themes".into(),
            content: "Dark editor chrome; avoid purple-on-white defaults.".into(),
            importance: Some(60),
            confidence: Some(80),
            tags: vec![],
            source: Some("user_request".into()),
        })
        .expect("create theme");

    // Duplicate kind must be rejected — preferences stay intentional and bounded.
    let duplicate = app.create_personal_memory(&CreatePersonalMemoryRequest {
        kind: PersonalMemoryKind::PreferredName,
        summary: "Other name".into(),
        content: "Someone else".into(),
        importance: None,
        confidence: None,
        tags: vec![],
        source: None,
    });
    assert!(duplicate.is_err());

    let updated = app
        .update_personal_memory(&UpdatePersonalMemoryRequest {
            memory_id: name.id.as_str().to_string(),
            summary: None,
            content: Some("Chuck".into()),
            importance: Some(95),
            confidence: None,
            tags: None,
        })
        .expect("update name");
    assert_eq!(updated.content, "Chuck");

    // Persistence across restart.
    drop(app);
    let app = Application::boot_with_data_dir(&data_dir).expect("reboot");
    let context = app.personal_context().expect("personal context");
    assert_eq!(context.entry_count(), 5);
    assert_eq!(context.preferred_name[0].content, "Chuck");
    assert_eq!(context.writing_style[0].id, writing.id);
    assert_eq!(context.code_style[0].id, code.id);
    assert_eq!(context.favorite_editor[0].id, editor.id);
    assert_eq!(context.preferred_theme[0].id, theme.id);

    // Planner retrieves personal memory automatically (no auto-create).
    let planner = app.container().resolve::<Planner>().expect("planner");
    let before = app.personal_context().expect("before").entry_count();
    let _ = planner
        .handle(UserRequest::new("list /tmp"))
        .expect("request with personal retrieve");
    let after = app.personal_context().expect("after").entry_count();
    assert_eq!(
        before, after,
        "handling requests must not auto-create personal memories"
    );

    let engine = app
        .container()
        .resolve::<Arc<MemoryEngine>>()
        .expect("memory engine");
    let retrieved = engine
        .retrieve(&jaymi_memory::MemoryQuery {
            scope: Some(jaymi_memory::MemoryScope::Personal),
            limit: Some(16),
            ..jaymi_memory::MemoryQuery::default()
        })
        .expect("retrieve personal");
    assert!(retrieved.iter().any(|record| record.content == "Chuck"));
    assert!(retrieved
        .iter()
        .any(|record| record.content.contains("Concise, technical")));

    app.delete_personal_memory(theme.id.as_str())
        .expect("delete theme");
    let trimmed = app.personal_context().expect("trimmed");
    assert_eq!(trimmed.preferred_theme.len(), 0);
    assert_eq!(trimmed.entry_count(), 4);
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-memory-it-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
