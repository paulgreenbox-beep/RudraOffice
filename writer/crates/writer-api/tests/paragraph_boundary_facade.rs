use writer_api::core::{
    BlockId, BlockKind, BlockSnapshot, Cursor, DocumentSnapshot, InlineRun, ParagraphStyle,
    Revision, Selection, TextBlockSnapshot, TextContainerId, TextOffset, TextStyle,
};
use writer_api::{ApiError, WriterApi};

fn paragraph(id: u64, runs: Vec<InlineRun>) -> BlockSnapshot {
    let text = runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<Vec<_>>()
        .concat();
    BlockSnapshot::Text(TextBlockSnapshot {
        id: BlockId::new(id),
        kind: BlockKind::Paragraph,
        paragraph_style: ParagraphStyle::default(),
        text,
        runs,
    })
}

fn styled_two_paragraph_snapshot() -> DocumentSnapshot {
    DocumentSnapshot {
        revision: Revision::new(0),
        blocks: vec![
            paragraph(
                10,
                vec![
                    InlineRun::new(
                        "А🙂",
                        TextStyle::default()
                            .with_font_family(Some("Arial".to_owned()))
                            .with_bold(true),
                    ),
                    InlineRun::new("БВ", TextStyle::default().with_italic(true)),
                ],
            ),
            paragraph(
                20,
                vec![
                    InlineRun::new("Г", TextStyle::default().with_underline(true)),
                    InlineRun::new(
                        "🚀Д",
                        TextStyle::default()
                            .with_font_family(Some("Calibri".to_owned()))
                            .with_strikethrough(true),
                    ),
                ],
            ),
        ],
    }
}

fn text_block(block: &BlockSnapshot) -> &TextBlockSnapshot {
    block.text_block().expect("fixture block must be text")
}

fn merged_block(left: &TextBlockSnapshot, right: &TextBlockSnapshot) -> BlockSnapshot {
    let mut runs = left.runs.clone();
    runs.extend(right.runs.iter().cloned());
    BlockSnapshot::Text(TextBlockSnapshot {
        id: left.id,
        kind: left.kind.clone(),
        paragraph_style: left.paragraph_style.clone(),
        text: format!("{}{}", left.text, right.text),
        runs,
    })
}

fn cursor(block_id: BlockId, offset: usize) -> Cursor {
    Cursor::new(TextContainerId::Block(block_id), TextOffset::new(offset))
}

#[test]
fn delete_backward_at_paragraph_start_merges_and_undo_redo_restores_exact_state() {
    let mut api = WriterApi::from_snapshot(styled_two_paragraph_snapshot())
        .expect("styled fixture must load through WriterApi");
    let original = api.document();
    let left = text_block(&original.blocks[0]);
    let right = text_block(&original.blocks[1]);
    let left_id = left.id;
    let right_id = right.id;
    let left_len = left.text.chars().count();
    let expected_joined = vec![merged_block(left, right)];
    let original_blocks = original.blocks.clone();
    let original_selection = Selection::collapsed(cursor(right_id, 0));
    api.set_selection(original_selection)
        .expect("second paragraph start is a valid cursor");

    assert!(
        left.text.len() > left_len,
        "fixture must include non-BMP text"
    );
    api.delete_backward()
        .expect("backward deletion at paragraph start must join paragraphs");

    assert_eq!(api.document().blocks, expected_joined);
    assert_eq!(
        api.selection(),
        Selection::collapsed(cursor(left_id, left_len)),
        "the caret must use scalar offsets and the surviving left block ID"
    );
    let joined_blocks = api.document().blocks;
    let joined_selection = api.selection();

    assert!(api.undo().expect("undo should succeed"));
    assert_eq!(api.document().blocks, original_blocks);
    assert_eq!(api.selection(), original_selection);
    assert!(api.can_redo());

    assert!(api.redo().expect("redo should succeed"));
    assert_eq!(api.document().blocks, joined_blocks);
    assert_eq!(api.selection(), joined_selection);
}

#[test]
fn delete_forward_at_paragraph_end_merges_next_paragraph_with_scalar_caret() {
    let mut api = WriterApi::from_snapshot(styled_two_paragraph_snapshot())
        .expect("styled fixture must load through WriterApi");
    let original = api.document();
    let left = text_block(&original.blocks[0]);
    let right = text_block(&original.blocks[1]);
    let left_id = left.id;
    let left_len = left.text.chars().count();
    let expected_joined = vec![merged_block(left, right)];
    api.set_selection(Selection::collapsed(cursor(left_id, left_len)))
        .expect("first paragraph end is a valid cursor");

    assert!(
        left.text.len() > left_len,
        "fixture must include non-BMP text"
    );
    api.delete_forward()
        .expect("forward deletion at paragraph end must join paragraphs");

    assert_eq!(api.document().blocks, expected_joined);
    assert_eq!(
        api.selection(),
        Selection::collapsed(cursor(left_id, left_len)),
        "the caret must remain at the scalar boundary in the surviving left block"
    );
}

fn page_break_snapshot() -> DocumentSnapshot {
    DocumentSnapshot {
        revision: Revision::new(0),
        blocks: vec![
            paragraph(
                30,
                vec![InlineRun::new("До", TextStyle::default().with_bold(true))],
            ),
            BlockSnapshot::Text(TextBlockSnapshot {
                id: BlockId::new(31),
                kind: BlockKind::PageBreak,
                paragraph_style: ParagraphStyle::default(),
                text: String::new(),
                runs: Vec::new(),
            }),
            paragraph(
                32,
                vec![InlineRun::new(
                    "После🙂",
                    TextStyle::default().with_italic(true),
                )],
            ),
        ],
    }
}

fn assert_page_break_noop_preserves_redo(
    boundary: Cursor,
    delete: fn(&mut WriterApi) -> Result<(), ApiError>,
) {
    let mut api = WriterApi::from_snapshot(page_break_snapshot())
        .expect("page-break fixture must load through WriterApi");
    let left_id = BlockId::new(30);
    api.set_selection(Selection::collapsed(cursor(left_id, 0)))
        .expect("fixture cursor must be valid");
    api.insert_text("Я")
        .expect("fixture edit should create a redoable history entry");
    let redo_blocks = api.document().blocks;
    let redo_selection = api.selection();
    assert!(api.undo().expect("undo should create a redo branch"));
    api.set_selection(Selection::collapsed(boundary))
        .expect("protected boundary cursor must be valid");
    let protected_blocks = api.document().blocks;
    let protected_selection = api.selection();
    assert!(!api.can_undo());
    assert!(api.can_redo());

    delete(&mut api).expect("deletion across a page break is a safe no-op");

    assert_eq!(api.document().blocks, protected_blocks);
    assert_eq!(api.selection(), protected_selection);
    assert!(!api.can_undo(), "a protected no-op must not add history");
    assert!(
        api.can_redo(),
        "a protected no-op must preserve the redo branch"
    );
    assert!(api.redo().expect("the preserved redo branch should apply"));
    assert_eq!(api.document().blocks, redo_blocks);
    assert_eq!(api.selection(), redo_selection);
}

#[test]
fn page_break_boundaries_are_noops_and_preserve_redo() {
    let left_id = BlockId::new(30);
    let right_id = BlockId::new(32);
    let left_len = "До".chars().count();

    assert_page_break_noop_preserves_redo(cursor(right_id, 0), WriterApi::delete_backward);
    assert_page_break_noop_preserves_redo(cursor(left_id, left_len), WriterApi::delete_forward);
}
