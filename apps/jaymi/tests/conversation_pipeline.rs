//! Integration test for Slice 4 conversation → Planner pipeline.

use jaymi::Application;

#[test]
fn conversation_message_flows_through_planner() {
    let app = Application::boot().expect("boot");
    let response = app
        .send_message("Hello Jaymi")
        .expect("send_message through planner");

    assert_eq!(
        response.capability.map(|capability| capability.id()),
        Some("chat")
    );
    assert!(response.tool_id.is_none());
    assert!(response.assistant_text().contains("Hello Jaymi"));
}

#[test]
fn conversation_can_still_list_and_read_via_text() {
    let dir = std::env::temp_dir().join(format!(
        "jaymi-conversation-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("note.md");
    std::fs::write(&file, "# Note\n\nBody").unwrap();

    let app = Application::boot().expect("boot");

    let list = app
        .send_message(format!("list {}", dir.display()))
        .expect("list via conversation");
    assert_eq!(
        list.capability.map(|capability| capability.id()),
        Some("search")
    );
    assert!(!list.entries.is_empty());

    let read = app
        .send_message(format!("read {}", file.display()))
        .expect("read via conversation");
    assert_eq!(
        read.capability.map(|capability| capability.id()),
        Some("read_content")
    );
    assert!(read.content.is_some());
    assert!(read.assistant_text().contains("Note"));
}
