use writer_api::{
    WriterApi,
    core::{BlockId, Command, Cursor, Selection, TextContainerId, TextOffset, TextRange},
};

#[test]
fn transactional_selection_and_unicode_replace_share_one_history_entry() {
    let mut api = WriterApi::new();
    api.insert_text("Старт🙂Финиш").unwrap();
    api.editor_mut().clear_history();
    let before = api.document();
    let selection_before = api.selection();
    let container = TextContainerId::Block(BlockId::new(1));
    let mut transaction = api.editor_mut().begin_transaction();
    transaction
        .set_selection(Selection::new(
            Cursor::new(container, TextOffset::new(5)),
            Cursor::new(container, TextOffset::new(6)),
        ))
        .unwrap();
    transaction
        .execute(Command::ReplaceRange {
            range: TextRange::new(TextOffset::new(5), TextOffset::new(6)),
            text: "🔥".to_owned(),
        })
        .unwrap();
    transaction.commit().unwrap();

    assert_eq!(api.document().blocks[0].text(), Some("Старт🔥Финиш"));
    assert_eq!(api.document().revision.get(), before.revision.get() + 1);
    assert!(api.undo().unwrap());
    assert_eq!(api.document().blocks, before.blocks);
    assert_eq!(api.selection(), selection_before);
    assert!(!api.can_undo());
    assert!(api.redo().unwrap());
    assert_eq!(api.document().blocks[0].text(), Some("Старт🔥Финиш"));
    assert_eq!(
        api.selection(),
        Selection::collapsed(Cursor::new(container, TextOffset::new(6)))
    );
    assert!(!api.can_redo());
}

#[test]
fn transactional_rejected_selection_and_rollback_preserve_api_state() {
    let mut api = WriterApi::new();
    api.insert_text("A🙂B").unwrap();
    api.editor_mut().clear_history();
    let before = api.document();
    let selection_before = api.selection();
    let container = TextContainerId::Block(BlockId::new(1));
    let mut transaction = api.editor_mut().begin_transaction();
    assert!(
        transaction
            .set_selection(Selection::collapsed(Cursor::new(
                container,
                TextOffset::new(4),
            )))
            .is_err()
    );
    assert_eq!(transaction.selection(), selection_before);
    transaction
        .set_selection(Selection::collapsed(Cursor::new(
            container,
            TextOffset::new(0),
        )))
        .unwrap();
    transaction
        .execute(Command::InsertText {
            text: "отмена ".to_owned(),
        })
        .unwrap();
    transaction.rollback();

    assert_eq!(api.document(), before);
    assert_eq!(api.selection(), selection_before);
    assert!(!api.can_undo());
    assert!(!api.can_redo());
}
