use writer_core::{Command, Editor, SearchQuery};

#[test]
fn longer_unicode_query_does_not_panic_or_mutate_document() {
    let mut editor = Editor::new();
    editor
        .execute(Command::InsertText {
            text: "Ж🙂".to_owned(),
        })
        .expect("insert synthetic Unicode text");
    let before = editor.snapshot();

    let matches = editor.find("Ж🙂длиннее").expect("bounded search");
    assert!(matches.is_empty());
    let matches = editor
        .find(SearchQuery::new("Ж🙂длиннее").ascii_case_insensitive(true))
        .expect("bounded case-insensitive search");
    assert!(matches.is_empty());
    assert_eq!(
        editor
            .replace_all("Ж🙂длиннее", "replacement")
            .expect("bounded replacement"),
        0
    );
    assert_eq!(editor.snapshot(), before);
}

#[test]
fn longer_query_than_empty_document_is_an_empty_result() {
    let editor = Editor::new();
    let matches = editor
        .find("query longer than empty")
        .expect("bounded search");
    assert!(matches.is_empty());
}

#[test]
fn exact_length_unicode_query_still_matches() {
    let mut editor = Editor::new();
    editor
        .execute(Command::InsertText {
            text: "Ж🙂".to_owned(),
        })
        .expect("insert synthetic Unicode text");
    assert_eq!(editor.find("Ж🙂").expect("exact-length search").len(), 1);
}
