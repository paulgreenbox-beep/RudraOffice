use writer_core::*;

fn paragraph(id: u64, runs: Vec<InlineRun>) -> BlockSnapshot {
    BlockSnapshot::Text(TextBlockSnapshot {
        id: BlockId::new(id),
        kind: BlockKind::Paragraph,
        paragraph_style: ParagraphStyle::default(),
        text: runs.iter().map(|run| run.text.as_str()).collect(),
        runs,
    })
}

fn plain(id: u64, text: &str) -> BlockSnapshot {
    paragraph(id, vec![InlineRun::new(text, TextStyle::default())])
}

fn cursor(id: u64, offset: usize) -> Selection {
    Selection::collapsed(Cursor::new(
        TextContainerId::Block(BlockId::new(id)),
        TextOffset::new(offset),
    ))
}

fn editor(blocks: Vec<BlockSnapshot>) -> Editor {
    let mut editor = Editor::new();
    editor
        .replace_document(DocumentSnapshot {
            blocks,
            revision: Revision::new(0),
        })
        .unwrap();
    editor.clear_history();
    editor
}

fn styled_paragraphs() -> Vec<BlockSnapshot> {
    let italic = TextStyle::default().with_italic(true);
    let mut left = paragraph(
        10,
        vec![
            InlineRun::new("Ж", TextStyle::default().with_bold(true)),
            InlineRun::new("🙂", italic.clone()),
        ],
    );
    let mut right = paragraph(
        20,
        vec![
            InlineRun::new("Б", italic),
            InlineRun::new("\u{301}", TextStyle::default().with_underline(true)),
            InlineRun::new(
                "終",
                TextStyle::default().with_foreground(Some(Rgba8::new(12, 34, 56, 255))),
            ),
        ],
    );
    if let BlockSnapshot::Text(block) = &mut left {
        block.paragraph_style = ParagraphStyle::default()
            .with_alignment(Alignment::Center)
            .with_indents(40, 20, -10)
            .with_spacing(5, 9);
    }
    if let BlockSnapshot::Text(block) = &mut right {
        block.paragraph_style = ParagraphStyle::default()
            .with_alignment(Alignment::Right)
            .with_line_spacing(LineSpacing::Double);
    }
    vec![left, right]
}

fn assert_styled_join(command: Command, selection: Selection) {
    let before_blocks = styled_paragraphs();
    let mut editor = editor(before_blocks.clone());
    editor.set_selection(selection).unwrap();
    let revision_before = editor.revision().get();

    editor.execute(command).unwrap();
    let joined = editor.snapshot();
    let block = joined.blocks[0].text_block().unwrap();
    assert_eq!(joined.blocks.len(), 1);
    assert_eq!(block.id, BlockId::new(10));
    assert_eq!(block.kind, BlockKind::Paragraph);
    assert_eq!(block.text, "Ж🙂Б\u{301}終");
    assert_eq!(
        block.paragraph_style,
        before_blocks[0].text_block().unwrap().paragraph_style
    );
    let expected_runs: Vec<_> = before_blocks
        .iter()
        .flat_map(|block| block.text_block().unwrap().runs.clone())
        .collect();
    assert_eq!(
        block.runs, expected_runs,
        "retain exact styled run boundaries"
    );
    assert_eq!(
        editor.selection(),
        cursor(10, 2),
        "caret uses scalar offsets"
    );
    assert_eq!(editor.revision().get(), revision_before + 1);
    joined.validate().unwrap();
    assert!(editor.can_undo());
    assert!(!editor.can_redo());

    assert!(editor.undo().unwrap());
    assert_eq!(editor.snapshot().blocks, before_blocks);
    assert_eq!(editor.selection(), selection);
    assert!(
        !editor.can_undo(),
        "one boundary command creates one history entry"
    );
    assert!(editor.can_redo());
    assert!(editor.redo().unwrap());
    assert_eq!(editor.snapshot().blocks, joined.blocks);
    assert_eq!(editor.selection(), cursor(10, 2));
    assert!(!editor.can_redo());
}

#[test]
fn backspace_joins_styled_unicode_paragraphs_in_one_history_entry() {
    assert_styled_join(Command::DeleteBackward, cursor(20, 0));
}

#[test]
fn delete_joins_styled_unicode_paragraphs_in_one_history_entry() {
    assert_styled_join(Command::DeleteForward, cursor(10, 2));
}

#[test]
fn empty_paragraphs_join_without_losing_left_metadata_or_runs() {
    for (left_text, right_text) in [("", "Ж🙂"), ("Ж🙂", ""), ("", "")] {
        for backwards in [true, false] {
            let mut left = paragraph(
                10,
                vec![InlineRun::new(
                    left_text,
                    TextStyle::default().with_bold(true),
                )],
            );
            if let BlockSnapshot::Text(block) = &mut left {
                block.paragraph_style = ParagraphStyle::default().with_alignment(Alignment::Center);
            }
            let right = paragraph(
                20,
                vec![InlineRun::new(
                    right_text,
                    TextStyle::default().with_italic(true),
                )],
            );
            let before = vec![left, right];
            let mut editor = editor(before.clone());
            let at = left_text.chars().count();
            let selection = if backwards {
                cursor(20, 0)
            } else {
                cursor(10, at)
            };
            editor.set_selection(selection).unwrap();
            editor
                .execute(if backwards {
                    Command::DeleteBackward
                } else {
                    Command::DeleteForward
                })
                .unwrap();
            let after = editor.snapshot();
            assert_eq!(after.blocks.len(), 1);
            let merged = after.blocks[0].text_block().unwrap();
            assert_eq!(merged.text, format!("{left_text}{right_text}"));
            assert_eq!(merged.id, BlockId::new(10));
            assert_eq!(merged.paragraph_style.alignment(), Alignment::Center);
            assert_eq!(merged.runs.len(), 2, "even empty styled runs survive");
            assert_eq!(merged.runs[0], before[0].text_block().unwrap().runs[0]);
            assert_eq!(merged.runs[1], before[1].text_block().unwrap().runs[0]);
            assert_eq!(editor.selection(), cursor(10, at));
            assert!(editor.undo().unwrap());
            assert_eq!(editor.snapshot().blocks, before);
            assert_eq!(editor.selection(), selection);
            assert!(!editor.can_undo());
        }
    }
}

fn assert_noop_preserves_redo(mut editor: Editor, selection: Selection, command: Command) {
    editor
        .execute(Command::InsertText {
            text: "履歴".into(),
        })
        .unwrap();
    let redo_blocks = editor.snapshot().blocks;
    assert!(editor.undo().unwrap());
    editor.set_selection(selection).unwrap();
    let before = editor.snapshot();
    editor.execute(command).unwrap();
    assert_eq!(editor.snapshot(), before, "no-op must not advance revision");
    assert_eq!(editor.selection(), selection);
    assert!(!editor.can_undo());
    assert!(editor.can_redo(), "no-op must retain the redo branch");
    assert!(editor.redo().unwrap());
    assert_eq!(editor.snapshot().blocks, redo_blocks);
}

#[test]
fn document_edges_are_noops_and_preserve_redo() {
    let blocks = vec![plain(10, "Л🙂"), plain(20, "П")];
    assert_noop_preserves_redo(
        editor(blocks.clone()),
        cursor(10, 0),
        Command::DeleteBackward,
    );
    assert_noop_preserves_redo(editor(blocks), cursor(20, 1), Command::DeleteForward);
}

#[test]
fn special_text_kinds_are_boundaries_in_both_directions() {
    for kind in [
        BlockKind::Heading(HeadingLevel::new(1).unwrap()),
        BlockKind::ListItem {
            list_id: ListId::new(3),
            kind: ListKind::Bullet,
        },
        BlockKind::PageBreak,
    ] {
        for special_left in [true, false] {
            let mut blocks = vec![plain(10, "Л"), plain(20, "П")];
            if let BlockSnapshot::Text(block) = &mut blocks[usize::from(!special_left)] {
                block.kind = kind.clone();
            }
            assert_noop_preserves_redo(
                editor(blocks.clone()),
                cursor(20, 0),
                Command::DeleteBackward,
            );
            assert_noop_preserves_redo(editor(blocks), cursor(10, 1), Command::DeleteForward);
        }
    }
}

fn table() -> BlockSnapshot {
    BlockSnapshot::Table(TableSnapshot {
        id: TableId::new(50),
        columns: vec![
            TableColumnSnapshot {
                id: ColumnId::new(60),
                width_half_points: 1440,
            },
            TableColumnSnapshot {
                id: ColumnId::new(61),
                width_half_points: 1440,
            },
        ],
        rows: vec![TableRowSnapshot {
            id: RowId::new(70),
            cells: vec![
                TableCellSnapshot {
                    id: CellId::new(80),
                    text: "Ж🙂".into(),
                    runs: vec![InlineRun::new("Ж🙂", TextStyle::default())],
                    style: CellStyle::default(),
                    alignment: CellAlignment::default(),
                },
                TableCellSnapshot {
                    id: CellId::new(81),
                    text: String::new(),
                    runs: Vec::new(),
                    style: CellStyle::default(),
                    alignment: CellAlignment::default(),
                },
            ],
        }],
    })
}

#[test]
fn tables_and_images_are_not_skipped_when_joining_paragraphs() {
    let image = BlockSnapshot::Image(ImageSnapshot {
        id: ImageId::new(50),
        payload: ImagePayload::new(ImageFormat::Png, PixelSize::new(1, 1).unwrap(), vec![1])
            .unwrap(),
        layout: ImageLayout::default(),
    });
    for barrier in [table(), image] {
        let blocks = vec![plain(10, "Л"), barrier, plain(20, "П")];
        assert_noop_preserves_redo(
            editor(blocks.clone()),
            cursor(20, 0),
            Command::DeleteBackward,
        );
        assert_noop_preserves_redo(editor(blocks), cursor(10, 1), Command::DeleteForward);
    }
}

#[test]
fn cell_boundaries_never_merge_cells_or_neighboring_blocks() {
    for (cell_id, length) in [(80, 2), (81, 0)] {
        for (offset, command) in [
            (0, Command::DeleteBackward),
            (length, Command::DeleteForward),
        ] {
            let selection = Selection::collapsed(Cursor::new(
                TextContainerId::Cell(CellId::new(cell_id)),
                TextOffset::new(offset),
            ));
            assert_noop_preserves_redo(editor(vec![table()]), selection, command);
        }
    }
}

#[test]
fn deletion_inside_a_paragraph_and_of_a_selection_stays_scalar_aware() {
    for (selection, command) in [
        (cursor(10, 2), Command::DeleteBackward),
        (cursor(10, 1), Command::DeleteForward),
        (
            Selection::new(
                Cursor::new(TextContainerId::Block(BlockId::new(10)), TextOffset::new(1)),
                Cursor::new(TextContainerId::Block(BlockId::new(10)), TextOffset::new(2)),
            ),
            Command::DeleteBackward,
        ),
    ] {
        let before = vec![plain(10, "А🙂Б"), plain(20, "В")];
        let mut editor = editor(before.clone());
        editor.set_selection(selection).unwrap();
        editor.execute(command).unwrap();
        assert_eq!(editor.snapshot().blocks.len(), 2);
        assert_eq!(editor.snapshot().blocks[0].text(), Some("АБ"));
        assert_eq!(editor.selection(), cursor(10, 1));
        assert!(editor.undo().unwrap());
        assert_eq!(editor.snapshot().blocks, before);
    }
}

#[test]
fn paragraph_join_does_not_reuse_removed_block_ids_after_history() {
    for (selection, command) in [
        (cursor(20, 0), Command::DeleteBackward),
        (cursor(10, 1), Command::DeleteForward),
    ] {
        let mut editor = editor(vec![plain(10, "Л"), plain(20, "П")]);
        editor.set_selection(selection).unwrap();
        editor.execute(command).unwrap();
        assert!(editor.undo().unwrap());
        assert!(editor.redo().unwrap());
        editor.set_selection(cursor(10, 1)).unwrap();
        editor.execute(Command::SplitBlock).unwrap();
        let blocks = editor.snapshot().blocks;
        assert_eq!(blocks[0].id(), 10);
        assert!(blocks[1].id() > 20);
        assert_eq!(blocks[0].text(), Some("Л"));
        assert_eq!(blocks[1].text(), Some("П"));
    }
}

#[test]
fn boundary_join_uses_existing_transaction_commit_and_rollback() {
    let before = vec![plain(10, "Л"), plain(20, "П")];
    let mut editor = editor(before.clone());
    editor.set_selection(cursor(20, 0)).unwrap();
    let baseline = editor.snapshot();
    let mut transaction = editor.begin_transaction();
    transaction.execute(Command::DeleteBackward).unwrap();
    assert_eq!(transaction.selection(), cursor(10, 1));
    transaction.rollback();
    assert_eq!(editor.snapshot(), baseline);
    assert_eq!(editor.selection(), cursor(20, 0));
    assert!(!editor.can_undo());
    let mut transaction = editor.begin_transaction();
    transaction.execute(Command::DeleteBackward).unwrap();
    transaction.commit().unwrap();
    assert_eq!(editor.snapshot().blocks.len(), 1);
    assert!(editor.undo().unwrap());
    assert_eq!(editor.snapshot().blocks, before);
    assert!(!editor.can_undo());
}

#[test]
fn explicit_merge_with_previous_keeps_its_broader_legacy_contract() {
    let mut blocks = vec![plain(10, "Л"), plain(20, "П🙂")];
    if let BlockSnapshot::Text(block) = &mut blocks[0] {
        block.kind = BlockKind::Heading(HeadingLevel::new(2).unwrap());
    }
    let mut editor = editor(blocks);
    editor.set_selection(cursor(20, 1)).unwrap();
    editor.execute(Command::MergeWithPrevious).unwrap();
    let snapshot = editor.snapshot();
    assert_eq!(snapshot.blocks.len(), 1);
    assert_eq!(snapshot.blocks[0].text(), Some("ЛП🙂"));
    assert_eq!(
        snapshot.blocks[0].text_block().unwrap().kind,
        BlockKind::Heading(HeadingLevel::new(2).unwrap())
    );
    assert_eq!(editor.selection(), cursor(10, 2));
}
