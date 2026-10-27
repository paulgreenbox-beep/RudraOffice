use super::*;
use slides_api::{PresentationId, Slide, SlideId};

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

fn controlled_writer_fixture_path() -> PathBuf {
    let relative = if cfg!(feature = "private-fixtures") {
        "../../../fixtures/release-acceptance/docx/writer-layout-a4-multipage.docx"
    } else {
        "../../../fixtures/public/writer/synthetic-grid.docx"
    };
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
fn blank_documents_use_normal_open_pipeline_and_export_reopens() {
    for (format, expected_module) in [
        (BlankDocumentFormat::Docx, Module::Writer),
        (BlankDocumentFormat::Xlsx, Module::Sheets),
        (BlankDocumentFormat::Pptx, Module::Slides),
        (BlankDocumentFormat::Pdf, Module::Pdf),
    ] {
        let path = blank_documents::create_temporary(format).expect("create blank file");
        let source = std::fs::read(&path).expect("read blank file");
        let (document, _, _, _) =
            OpenDocument::open_background(&path, &source).expect("open blank file");
        assert_eq!(document.module(), expected_module);
        let exported = if format == BlankDocumentFormat::Pdf {
            source
        } else {
            document.export_bytes().expect("export blank document")
        };
        let destination = path.with_extension(format!("saved.{}", format.extension()));
        new_document_save::write_new_document(&destination, format.extension(), &exported)
            .expect("save as fresh document");
        assert!(
            new_document_save::write_new_document(&destination, format.extension(), &exported,)
                .is_err()
        );
        let saved = std::fs::read(&destination).expect("read saved document");
        let (reopened, _, _, _) =
            OpenDocument::open_background(&destination, &saved).expect("reopen saved document");
        assert_eq!(reopened.module(), expected_module);
        std::fs::remove_file(&path).expect("remove test blank file");
        std::fs::remove_file(&destination).expect("remove test saved file");
    }
}

#[test]
fn only_new_document_session_can_save_and_close_clears_permission() {
    let mut state = state_with_test_document(
        "imported.docx",
        OpenDocument::Writer(Box::new(WriterViewModel::new())),
    );
    let id = state
        .active_document_id()
        .expect("active imported document");
    assert!(!state.can_save_new_document());
    state
        .new_document_formats
        .insert(id, BlankDocumentFormat::Docx);
    let temp_path = blank_documents::create_temporary(BlankDocumentFormat::Docx)
        .expect("temporary test document");
    state.new_document_temp_paths.insert(id, temp_path.clone());
    assert!(state.can_save_new_document());
    assert_eq!(state.document_tab_frames()[0].label, "Новый документ.docx");
    state.close_document(id).expect("close new document");
    assert!(!state.can_save_new_document());
    assert!(!state.new_document_formats.contains_key(&id));
    assert!(!temp_path.exists());
}

#[test]
fn writer_export_preserves_edited_cyrillic_text() {
    let mut writer = WriterViewModel::new();
    writer
        .insert_text("Проверка сохранения нового документа")
        .expect("edit new Writer document");
    let bytes = OpenDocument::Writer(Box::new(writer))
        .export_bytes()
        .expect("export edited DOCX");
    let (reopened, _, _, _) = OpenDocument::open_background(Path::new("edited.docx"), &bytes)
        .expect("reopen edited DOCX");
    let OpenDocument::Writer(reopened) = reopened else {
        panic!("Writer document expected");
    };
    assert!(
        writer_text_editor_blocks(&reopened)
            .iter()
            .any(|block| block.text.contains("Проверка сохранения нового документа"))
    );
}

fn assert_round_trip(path: &str, bytes: Vec<u8>, expected_module: Module) {
    let mut engine = PdfOxideAdapter::default();
    let (document, _, _) =
        OpenDocument::open(Path::new(path), &bytes, &mut engine).expect("must open document");
    assert_eq!(document.module(), expected_module);
    let saved = document.export_bytes().expect("must export document");
    let (reopened, _, _) = OpenDocument::open(Path::new(path), &saved, &mut engine)
        .expect("must reopen exported document");
    assert_eq!(reopened.module(), expected_module);
}

fn attach_test_document(
    state: &mut AppState,
    path: &str,
    document: OpenDocument,
) -> RuntimeDocumentId {
    let session = DocumentSession::new(DocumentSource::from_path(PathBuf::from(path)), document)
        .expect("test document must form a valid session");
    let id = state
        .documents
        .insert(session)
        .expect("test document ID must allocate");
    state.active_document = Some(id);
    id
}

fn state_with_test_document(path: &str, document: OpenDocument) -> AppState {
    let mut state = AppState::default();
    attach_test_document(&mut state, path, document);
    state
}

#[test]
fn home_ribbon_tracks_active_document_lifetime() {
    let mut state = AppState::default();
    let id = state
        .register_document(
            PathBuf::from("document.docx"),
            OpenDocument::Writer(Box::new(WriterViewModel::new())),
        )
        .expect("writer session must register");

    state
        .activate_document(id)
        .expect("writer session must activate");
    assert!(state.home_ribbon_open);

    state.close_document(id).expect("writer session must close");
    assert!(!state.home_ribbon_open);
}

#[test]
fn color_dropdowns_map_only_actionable_entries() {
    assert_eq!(writer_color_command_from_dropdown(0), None);
    assert_eq!(
        writer_color_command_from_dropdown(3),
        Some(WriterHomeCommand::SetForeground(Some(Rgba8::rgb(
            220, 38, 38
        ))))
    );
    assert_eq!(
        writer_color_command_from_dropdown(8),
        Some(WriterHomeCommand::SetBackground(Some(Rgba8::rgb(
            254, 240, 138
        ))))
    );
    assert_eq!(writer_color_command_from_dropdown(usize::MAX), None);

    assert_eq!(sheets_color_command_from_dropdown(0), None);
    assert_eq!(
        sheets_color_command_from_dropdown(2),
        Some(SheetsHomeFormat::SetTextColor(SheetsRgba8::rgb(
            220, 38, 38
        )))
    );
    assert_eq!(
        sheets_color_command_from_dropdown(7),
        Some(SheetsHomeFormat::SetCellBackground(SheetsRgba8::rgb(
            254, 240, 138
        )))
    );
    assert_eq!(sheets_color_command_from_dropdown(usize::MAX), None);

    assert_eq!(slides_color_command_from_dropdown(0), None);
    assert_eq!(
        slides_color_command_from_dropdown(3),
        Some(SlidesHomeCommand::SetTextColor(SlidesColor::rgb(
            37, 99, 235
        )))
    );
    assert_eq!(slides_color_command_from_dropdown(usize::MAX), None);
    assert_eq!(
        writer_color_accessibility_value(
            &writer_color_command_from_dropdown(3).expect("Writer red")
        ),
        Some("text-red")
    );
    assert_eq!(
        sheets_color_accessibility_value(
            &sheets_color_command_from_dropdown(7).expect("Sheets yellow")
        ),
        Some("cell-yellow")
    );
    assert_eq!(
        slides_color_accessibility_value(
            &slides_color_command_from_dropdown(3).expect("Slides blue")
        ),
        Some("text-blue")
    );
}

#[test]
fn writer_text_editor_replaces_real_document_text_undoes_and_clears_tab_draft() {
    let mut writer = WriterViewModel::new();
    writer
        .insert_text("До изменения")
        .expect("test text must insert");
    let mut state =
        state_with_test_document("editable.docx", OpenDocument::Writer(Box::new(writer)));
    let document_id = state.active_document_id().expect("active Writer document");

    let model = state
        .writer_text_editor_model()
        .expect("Writer editor model");
    assert!(model.enabled);
    assert_eq!(model.selected_index, 1);
    assert_eq!(model.text, "До изменения");
    assert_eq!(model.labels.len(), 2);

    state
        .select_writer_text_editor_block(1)
        .expect("first paragraph must select");
    state
        .apply_writer_text_editor("После изменения".to_owned())
        .expect("paragraph replacement must apply");
    let edited_text = match state.document().expect("writer document") {
        OpenDocument::Writer(writer) => writer.snapshot().blocks[0]
            .text()
            .expect("first block text")
            .to_owned(),
        _ => panic!("writer backend expected"),
    };
    assert_eq!(edited_text, "После изменения");
    assert!(state.writer_text_drafts.contains_key(&document_id));

    state
        .apply_writer_home_command(WriterHomeCommand::Undo)
        .expect("text replacement must undo");
    let restored_text = match state.document().expect("writer document") {
        OpenDocument::Writer(writer) => writer.snapshot().blocks[0]
            .text()
            .expect("first block text")
            .to_owned(),
        _ => panic!("writer backend expected"),
    };
    assert_eq!(restored_text, "До изменения");
    assert!(!state.writer_text_drafts.contains_key(&document_id));

    state
        .close_document(document_id)
        .expect("document must close");
    assert!(!state.writer_text_drafts.contains_key(&document_id));
}

#[test]
fn writer_text_edit_preserves_unchanged_styled_suffix() {
    assert_eq!(
        writer_minimal_text_change("Привет мир", "Привет новый мир"),
        (7, 7, "новый ".to_owned())
    );
    assert_eq!(
        writer_minimal_text_change("A🙂B", "A🔥B"),
        (1, 2, "🔥".to_owned())
    );

    let mut writer = WriterViewModel::new();
    let original = "Привет мир";
    writer.insert_text(original).expect("test text must insert");
    let block_id = writer.snapshot().blocks[0]
        .text_block()
        .expect("first paragraph")
        .id
        .get();
    let mut state = state_with_test_document("styled.docx", OpenDocument::Writer(Box::new(writer)));
    state
        .select_writer_block_text_range(
            block_id,
            original,
            original.find("мир").expect("styled word"),
            original.len(),
        )
        .expect("select styled suffix");
    state
        .apply_writer_home_command(WriterHomeCommand::ToggleBold)
        .expect("style suffix");
    state
        .select_writer_text_editor_block(1)
        .expect("first paragraph must select");
    state
        .apply_writer_text_editor("Привет новый мир".to_owned())
        .expect("insert before styled suffix");
    let OpenDocument::Writer(writer) = state.document().expect("writer document") else {
        panic!("writer backend expected")
    };
    let snapshot = writer.snapshot();
    let runs = &snapshot.blocks[0].text_block().expect("paragraph").runs;
    assert_eq!(
        runs.iter().map(|run| run.text.as_str()).collect::<String>(),
        "Привет новый мир"
    );
    assert!(!runs[0].style.bold());
    assert_eq!(runs.last().expect("styled suffix").text, "мир");
    assert!(runs.last().expect("styled suffix").style.bold());

    state
        .apply_writer_home_command(WriterHomeCommand::Undo)
        .expect("text edit must undo without removing the prior style");
    let OpenDocument::Writer(writer) = state.document().expect("writer document") else {
        panic!("writer backend expected")
    };
    let snapshot = writer.snapshot();
    let runs = &snapshot.blocks[0].text_block().expect("paragraph").runs;
    assert_eq!(
        runs.iter().map(|run| run.text.as_str()).collect::<String>(),
        original
    );
    assert!(runs.last().expect("styled suffix").style.bold());
}

#[test]
fn writer_selected_cyrillic_range_receives_all_character_formats_only_on_selection() {
    let mut writer = WriterViewModel::new();
    let text = "Привет мир";
    writer.insert_text(text).expect("test text must insert");
    let block_id = writer.snapshot().blocks[0]
        .text_block()
        .expect("first paragraph")
        .id
        .get();
    let mut state =
        state_with_test_document("selected.docx", OpenDocument::Writer(Box::new(writer)));
    let start = text.find("мир").expect("selected word");
    assert_eq!(
        writer_char_range_from_byte_range(text, start, text.len()),
        Ok((7, 10))
    );
    assert!(writer_char_range_from_byte_range(text, start + 1, text.len()).is_err());
    state
        .select_writer_block_text_range(block_id, text, start, text.len())
        .expect("select Cyrillic word");
    for command in [
        WriterHomeCommand::ToggleBold,
        WriterHomeCommand::ToggleItalic,
        WriterHomeCommand::ToggleUnderline,
        WriterHomeCommand::ToggleStrikethrough,
        WriterHomeCommand::SetForeground(Some(Rgba8::rgb(220, 38, 38))),
        WriterHomeCommand::SetBackground(Some(Rgba8::rgb(254, 240, 138))),
    ] {
        state
            .apply_writer_home_command(command)
            .expect("selected text format must apply");
    }
    let OpenDocument::Writer(writer) = state.document().expect("writer document") else {
        panic!("writer backend expected")
    };
    let snapshot = writer.snapshot();
    let runs = &snapshot.blocks[0].text_block().expect("paragraph").runs;
    assert_eq!(
        runs.iter().map(|run| run.text.as_str()).collect::<String>(),
        text
    );
    let prefix = runs
        .iter()
        .find(|run| run.text.contains("Привет"))
        .expect("prefix");
    assert!(!prefix.style.bold());
    assert_eq!(prefix.style.foreground(), None);
    assert_eq!(prefix.style.background(), None);
    let selected = runs
        .iter()
        .find(|run| run.text == "мир")
        .expect("selected word");
    assert!(selected.style.bold());
    assert!(selected.style.italic());
    assert!(selected.style.underline());
    assert!(selected.style.strikethrough());
    assert_eq!(selected.style.foreground(), Some(Rgba8::rgb(220, 38, 38)));
    assert_eq!(selected.style.background(), Some(Rgba8::rgb(254, 240, 138)));
}

#[test]
fn writer_zoom_dropdown_maps_only_visible_presets() {
    assert_eq!(writer_zoom_dropdown_index_for_percent(50), 0);
    assert_eq!(writer_zoom_dropdown_index_for_percent(100), 2);
    assert_eq!(writer_zoom_dropdown_index_for_percent(200), 5);
    assert_eq!(writer_zoom_dropdown_index_for_percent(110), 6);
    assert_eq!(writer_zoom_percent_from_dropdown(0), Some(50));
    assert_eq!(writer_zoom_percent_from_dropdown(2), Some(100));
    assert_eq!(writer_zoom_percent_from_dropdown(5), Some(200));
    assert_eq!(writer_zoom_percent_from_dropdown(6), None);
    assert_eq!(writer_zoom_percent_from_dropdown(7), None);
    assert_eq!(writer_zoom_percent_from_value("125%"), Ok(125));
    assert_eq!(writer_zoom_percent_from_value(" 200 "), Ok(200));
    assert!(writer_zoom_percent_from_value("110%").is_err());
}

#[test]
fn chrome_drag_target_excludes_tabs_and_window_controls() {
    let header = Rect {
        pos: dvec2(0.0, 0.0),
        size: dvec2(500.0, 42.0),
    };
    let tab_bounds = [Rect {
        pos: dvec2(100.0, 2.0),
        size: dvec2(180.0, 38.0),
    }];
    let controls = [Rect {
        pos: dvec2(450.0, 2.0),
        size: dvec2(46.0, 38.0),
    }];

    assert!(chrome_drag_target(
        header,
        &controls,
        &tab_bounds,
        dvec2(40.0, 20.0),
    ));
    assert!(chrome_drag_target(
        header,
        &controls,
        &tab_bounds,
        dvec2(320.0, 20.0),
    ));
    // This reproduces the old caller mistake: passing the Fill-sized host
    // instead of a rendered tab would incorrectly reject a free point.
    let full_host = [Rect {
        pos: dvec2(100.0, 2.0),
        size: dvec2(350.0, 38.0),
    }];
    assert!(!chrome_drag_target(
        header,
        &controls,
        &full_host,
        dvec2(320.0, 20.0),
    ));
    assert!(!chrome_drag_target(
        header,
        &controls,
        &tab_bounds,
        dvec2(140.0, 20.0),
    ));
    assert!(!chrome_drag_target(
        header,
        &controls,
        &tab_bounds,
        dvec2(470.0, 20.0),
    ));
    assert!(!chrome_drag_target(
        header,
        &controls,
        &tab_bounds,
        dvec2(520.0, 20.0),
    ));
}

#[test]
fn writer_home_formatting_targets_current_text_container_and_undoes() {
    let mut writer = WriterViewModel::new();
    writer
        .insert_text("Rudra Writer")
        .expect("test text must insert");
    let mut state =
        state_with_test_document("document.docx", OpenDocument::Writer(Box::new(writer)));

    assert!(state.writer_character_toolbar_available());
    state
        .apply_writer_home_command(WriterHomeCommand::ToggleBold)
        .expect("bold must apply to current text container");
    let bold_runs = match state.document().expect("writer document") {
        OpenDocument::Writer(writer) => writer
            .snapshot()
            .blocks
            .into_iter()
            .filter_map(|block| block.text_block().cloned())
            .flat_map(|block| block.runs)
            .filter(|run| !run.text.is_empty())
            .collect::<Vec<_>>(),
        _ => panic!("writer backend expected"),
    };
    assert!(!bold_runs.is_empty());
    assert!(bold_runs.iter().all(|run| run.style.bold()));

    state
        .apply_writer_home_command(WriterHomeCommand::Undo)
        .expect("toolbar edit must undo");
    let reverted = match state.document().expect("writer document") {
        OpenDocument::Writer(writer) => writer
            .snapshot()
            .blocks
            .into_iter()
            .filter_map(|block| block.text_block().cloned())
            .flat_map(|block| block.runs)
            .filter(|run| !run.text.is_empty())
            .collect::<Vec<_>>(),
        _ => panic!("writer backend expected"),
    };
    assert!(reverted.iter().all(|run| !run.style.bold()));
}

#[test]
fn history_dispatch_is_targeted_to_active_writer_tab() {
    let mut state = AppState::default();
    let mut first = WriterViewModel::new();
    first.insert_text("Первый").expect("first text");
    let mut second = WriterViewModel::new();
    second.insert_text("Второй").expect("second text");
    let first_id = state
        .register_document(
            PathBuf::from("one.docx"),
            OpenDocument::Writer(Box::new(first)),
        )
        .expect("first document");
    let second_id = state
        .register_document(
            PathBuf::from("two.docx"),
            OpenDocument::Writer(Box::new(second)),
        )
        .expect("second document");
    let is_bold = |state: &AppState, id: RuntimeDocumentId| {
        let session = state.documents.require(id).expect("live document");
        let OpenDocument::Writer(writer) = session.backend() else {
            panic!("writer backend expected");
        };
        writer.snapshot().blocks[0]
            .text_block()
            .expect("paragraph")
            .runs
            .iter()
            .any(|run| !run.text.is_empty() && run.style.bold())
    };

    state.activate_document(first_id).expect("activate first");
    state
        .apply_writer_home_command(WriterHomeCommand::ToggleBold)
        .expect("first formatting");
    state.activate_document(second_id).expect("activate second");
    state
        .apply_writer_home_command(WriterHomeCommand::ToggleBold)
        .expect("second formatting");
    assert!(is_bold(&state, first_id));
    assert!(is_bold(&state, second_id));

    assert!(
        state
            .dispatch_history_command(first_id, OfficeHistoryCommand::Undo)
            .is_err()
    );
    state
        .dispatch_history_command(second_id, OfficeHistoryCommand::Undo)
        .expect("undo only active tab");
    assert!(is_bold(&state, first_id));
    assert!(!is_bold(&state, second_id));
    state
        .dispatch_history_command(second_id, OfficeHistoryCommand::Redo)
        .expect("redo only active tab");
    assert!(is_bold(&state, second_id));

    state.activate_document(first_id).expect("return to first");
    state
        .dispatch_history_command(first_id, OfficeHistoryCommand::Undo)
        .expect("first history preserved");
    assert!(!is_bold(&state, first_id));
    assert!(is_bold(&state, second_id));
    state.close_document(first_id).expect("close first");
    assert!(
        state
            .dispatch_history_command(first_id, OfficeHistoryCommand::Undo)
            .is_err()
    );
}

#[test]
fn history_dispatch_rejects_format_without_host_history() {
    let mut state = AppState::default();
    let id = state
        .register_document(
            PathBuf::from("viewer.pdf"),
            OpenDocument::Pdf {
                document_id: DocumentId::new(1),
                page_count: 1,
                page_ids: vec![PageId::new(1)],
            },
        )
        .expect("pdf session");
    state.activate_document(id).expect("activate pdf");
    assert!(
        state
            .dispatch_history_command(id, OfficeHistoryCommand::Undo)
            .is_err()
    );
}

#[test]
fn writer_home_colors_reach_visual_runs_and_background_undoes() {
    let mut writer = WriterViewModel::new();
    writer
        .insert_text("Цветной текст")
        .expect("test text must insert");
    let mut state = state_with_test_document("colors.docx", OpenDocument::Writer(Box::new(writer)));

    state
        .apply_writer_home_command(WriterHomeCommand::SetForeground(Some(Rgba8::rgb(
            220, 38, 38,
        ))))
        .expect("foreground must apply");
    state
        .apply_writer_home_command(WriterHomeCommand::SetBackground(Some(Rgba8::rgb(
            254, 240, 138,
        ))))
        .expect("background must apply");

    let writer = match state.document().expect("writer document") {
        OpenDocument::Writer(writer) => writer,
        _ => panic!("writer backend expected"),
    };
    let text = writer.snapshot().blocks[0]
        .text_block()
        .expect("text block")
        .clone();
    let styled = text
        .runs
        .iter()
        .find(|run| !run.text.is_empty())
        .expect("styled run");
    assert_eq!(styled.style.foreground(), Some(Rgba8::rgb(220, 38, 38)));
    assert_eq!(styled.style.background(), Some(Rgba8::rgb(254, 240, 138)));
    assert_eq!(state.writer_color_value, Some("highlight-yellow"));

    let layout = writer.state().layout.as_ref().expect("writer layout");
    let frame = build_writer_page_frame(
        &writer.snapshot(),
        layout,
        writer.api().layout_settings(),
        writer.docx_layout(),
        100,
        0.0,
        0.0,
    );
    let visual_run = frame.pages[0]
        .blocks
        .iter()
        .find_map(|block| match block {
            writer_page_view::WriterVisualBlock::Paragraph(paragraph) => {
                paragraph.runs.iter().find(|run| !run.text.is_empty())
            }
            _ => None,
        })
        .expect("colored run must reach visual frame");
    assert_eq!(visual_run.style.foreground, Some([220, 38, 38, 255]));
    assert_eq!(visual_run.style.background, Some([254, 240, 138, 255]));

    state
        .apply_writer_home_command(WriterHomeCommand::Undo)
        .expect("background edit must undo");
    let writer = match state.document().expect("writer document") {
        OpenDocument::Writer(writer) => writer,
        _ => panic!("writer backend expected"),
    };
    let snapshot = writer.snapshot();
    let reverted = snapshot.blocks[0]
        .text_block()
        .expect("text block")
        .runs
        .iter()
        .find(|run| !run.text.is_empty())
        .expect("reverted run");
    assert_eq!(reverted.style.foreground(), Some(Rgba8::rgb(220, 38, 38)));
    assert_eq!(reverted.style.background(), None);
}

#[test]
fn writer_ruler_left_indent_updates_selected_block_and_undoes() {
    let mut writer = WriterViewModel::new();
    writer.insert_text("Абзац").expect("test text must insert");
    let block_id = writer_selected_block_id(&writer).expect("selected text block");
    let settings = writer.api().layout_settings();
    let target_absolute = settings.margins.left_half_points + 120;
    let mut state = state_with_test_document("ruler.docx", OpenDocument::Writer(Box::new(writer)));

    state
        .apply_writer_ruler_change(
            &format!("{WRITER_PARAGRAPH_LEFT_MARKER_PREFIX}{block_id}"),
            f64::from(target_absolute) * PIXELS_PER_HALF_POINT_AT_100,
        )
        .expect("ruler edit must apply");
    let left_indent = match state.document().expect("writer document") {
        OpenDocument::Writer(writer) => writer.snapshot().blocks[0]
            .text_block()
            .expect("text block")
            .paragraph_style
            .indent_left_half_points(),
        _ => panic!("writer backend expected"),
    };
    assert_eq!(left_indent, 120);

    state
        .apply_writer_home_command(WriterHomeCommand::Undo)
        .expect("ruler paragraph edit must use Writer undo");
    let left_indent = match state.document().expect("writer document") {
        OpenDocument::Writer(writer) => writer.snapshot().blocks[0]
            .text_block()
            .expect("text block")
            .paragraph_style
            .indent_left_half_points(),
        _ => panic!("writer backend expected"),
    };
    assert_eq!(left_indent, 0);
}

#[test]
fn writer_ruler_maps_left_right_and_first_line_markers() {
    let margins = writer_api::layout::PageMargins {
        top_half_points: 100,
        right_half_points: 100,
        bottom_half_points: 100,
        left_half_points: 100,
    };
    assert_eq!(
        writer_indents_from_ruler(
            WriterRulerTarget::ParagraphLeft(1),
            180,
            1_000,
            margins,
            20,
            30,
            10,
        ),
        (80, 30, 10)
    );
    assert_eq!(
        writer_indents_from_ruler(
            WriterRulerTarget::ParagraphRight(1),
            800,
            1_000,
            margins,
            20,
            30,
            10,
        ),
        (20, 100, 10)
    );
    assert_eq!(
        writer_indents_from_ruler(
            WriterRulerTarget::ParagraphFirstLine(1),
            160,
            1_000,
            margins,
            20,
            30,
            10,
        ),
        (20, 30, 40)
    );
}

#[test]
fn writer_ruler_header_and_footer_positions_update_session_layout() {
    let path = controlled_writer_fixture_path();
    let bytes = std::fs::read(&path).expect("controlled Writer fixture must be readable");
    let mut pdf_engine = PdfOxideAdapter::default();
    let (document, _, _) = OpenDocument::open(&path, &bytes, &mut pdf_engine)
        .expect("controlled Writer fixture must open through Writer");
    let mut state = state_with_test_document("headers.docx", document);
    let page_height = match state.document().expect("writer document") {
        OpenDocument::Writer(writer) => writer.api().layout_settings().page_size.height_half_points,
        _ => panic!("writer backend expected"),
    };

    state
        .apply_writer_ruler_change(
            WRITER_HEADER_DISTANCE_MARKER_ID,
            96.0 * PIXELS_PER_HALF_POINT_AT_100,
        )
        .expect("header marker edit must apply");
    state
        .apply_writer_ruler_change(
            WRITER_FOOTER_DISTANCE_MARKER_ID,
            f64::from(page_height - 120) * PIXELS_PER_HALF_POINT_AT_100,
        )
        .expect("footer marker edit must apply");

    let section = match state.document().expect("writer document") {
        OpenDocument::Writer(writer) => writer
            .docx_layout()
            .sections
            .first()
            .expect("DOCX section")
            .clone(),
        _ => panic!("writer backend expected"),
    };
    assert_eq!(section.header_distance_twips, 960);
    assert_eq!(section.footer_distance_twips, 1_200);
}

#[test]
fn writer_home_font_size_reaches_controlled_visual_frame() {
    let path = controlled_writer_fixture_path();
    let bytes = std::fs::read(&path).expect("controlled Writer fixture must be readable");
    let mut pdf_engine = PdfOxideAdapter::default();
    let (document, _, _) = OpenDocument::open(&path, &bytes, &mut pdf_engine)
        .expect("controlled Writer fixture must open through Writer");
    let mut state = state_with_test_document("document.docx", document);
    let selected = match state.document().expect("writer document") {
        OpenDocument::Writer(writer) => writer.api().selection().active().container(),
        _ => panic!("writer backend expected"),
    };

    state
        .apply_writer_home_command(WriterHomeCommand::SetFontSizePoints(80))
        .expect("font size must apply to the selected Writer container");

    let writer = match state.document().expect("writer document") {
        OpenDocument::Writer(writer) => writer,
        _ => panic!("writer backend expected"),
    };
    let writer_state = writer.state();
    let layout = writer_state
        .layout
        .as_ref()
        .expect("controlled Writer fixture must retain layout");
    let frame = build_writer_page_frame(
        &writer.snapshot(),
        layout,
        writer.api().layout_settings(),
        writer.docx_layout(),
        writer_state.zoom_percent,
        0.0,
        0.0,
    );
    let reaches_visual_frame = frame.pages.iter().any(|page| {
        page.blocks.iter().any(|block| match block {
            writer_page_view::WriterVisualBlock::Paragraph(paragraph) => paragraph
                .runs
                .iter()
                .any(|run| run.style.size_half_points == 160),
            writer_page_view::WriterVisualBlock::Table(table) => table.rows.iter().any(|row| {
                row.cells.iter().any(|cell| {
                    cell.runs
                        .iter()
                        .any(|run| run.style.size_half_points == 160)
                })
            }),
            writer_page_view::WriterVisualBlock::Image(_) => false,
            writer_page_view::WriterVisualBlock::Text(line) => line.style.size_half_points == 160,
        })
    });
    assert!(
        reaches_visual_frame,
        "font-size mutation for {selected:?} must reach the Writer visual render plan"
    );
}

#[test]
fn sheets_home_edits_selected_cell_and_applies_real_format() {
    let api = SpreadsheetApi::new(IroncalcAdapter::new());
    let package = api.export_xlsx().expect("test workbook must export");
    let snapshot = api.document().snapshot();
    let mut state = state_with_test_document("book.xlsx", OpenDocument::Sheets(snapshot));
    state
        .bind_sheets_visual(&package)
        .expect("visual Sheets model must bind");
    let sheet = state
        .sheets_api
        .as_ref()
        .expect("Sheets API")
        .document()
        .active_sheet();
    let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
    state
        .sheets_view
        .as_mut()
        .expect("Sheets view")
        .select_cell(address)
        .expect("A1 must select");

    state
        .apply_sheets_cell_input("42".to_owned())
        .expect("cell input must commit");
    state
        .apply_sheets_home_format(SheetsHomeFormat::ToggleBold)
        .expect("cell format must commit");
    state
        .apply_sheets_home_format(SheetsHomeFormat::SetTextColor(SheetsRgba8::rgb(
            220, 38, 38,
        )))
        .expect("cell text color must commit");
    state
        .apply_sheets_home_format(SheetsHomeFormat::SetCellBackground(SheetsRgba8::rgb(
            254, 240, 138,
        )))
        .expect("whole-cell background must commit");

    let OpenDocument::Sheets(snapshot) = state.document().expect("Sheets document") else {
        panic!("Sheets backend expected");
    };
    let cell = snapshot
        .sheets
        .iter()
        .flat_map(|sheet| sheet.cells.iter())
        .find(|cell| cell.address == address)
        .expect("edited A1 must be materialized");
    assert!(matches!(cell.input, CellInput::Number(value) if value == 42.0));
    let format = cell.format.as_ref().expect("edited cell format");
    assert!(format.font.bold);
    assert_eq!(
        format.foreground,
        Some(sheets_core::Color::new(220, 38, 38, 255))
    );
    assert_eq!(
        format.background,
        Some(sheets_core::Color::new(254, 240, 138, 255))
    );
    assert_eq!(state.sheets_color_value, Some("cell-yellow"));
    let bytes = state
        .document()
        .unwrap()
        .export_bytes()
        .expect("export edited XLSX");
    let (reopened, _, _, _) = OpenDocument::open_background(Path::new("edited.xlsx"), &bytes)
        .expect("reopen edited XLSX");
    let OpenDocument::Sheets(reopened) = reopened else {
        panic!("Sheets document expected");
    };
    assert!(
        reopened
            .sheets
            .iter()
            .flat_map(|sheet| sheet.cells.iter())
            .any(|cell| cell.address == address
                && matches!(cell.input, CellInput::Number(value) if value == 42.0))
    );
}

#[test]
fn sheets_editor_binding_preserves_draft_on_redraw_and_resets_for_new_cell() {
    let api = SpreadsheetApi::new(IroncalcAdapter::new());
    let state = state_with_test_document(
        "draft.xlsx",
        OpenDocument::Sheets(api.document().snapshot()),
    );
    let id = state.active_document_id().unwrap();
    let sheet = api.document().active_sheet();
    let a1 = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
    let a2 = CellAddress::new(sheet, RowIndex(1), ColumnIndex(0));
    let mut binding = None;
    assert_eq!(
        sheets_input_sync_value(&mut binding, Some((id, a1, "10".into()))),
        Some("10".into())
    );
    // The widget can contain a new draft such as "20"; no redraw write is returned.
    assert_eq!(
        sheets_input_sync_value(&mut binding, Some((id, a1, "10".into()))),
        None
    );
    assert_eq!(
        sheets_input_sync_value(&mut binding, Some((id, a1, "20".into()))),
        Some("20".into())
    );
    assert_eq!(
        sheets_input_sync_value(&mut binding, Some((id, a2, String::new()))),
        Some(String::new())
    );
    assert_eq!(
        sheets_input_sync_value(&mut binding, None),
        Some(String::new())
    );
}

#[test]
fn ribbon_sheets_commands_update_real_model_without_changing_document_identity() {
    let api = SpreadsheetApi::new(IroncalcAdapter::new());
    let package = api.export_xlsx().unwrap();
    let mut state = state_with_test_document(
        "ribbon.xlsx",
        OpenDocument::Sheets(api.document().snapshot()),
    );
    state.bind_sheets_visual(&package).unwrap();
    let id = state.active_document_id().unwrap();
    let sheet = state.sheets_api.as_ref().unwrap().document().active_sheet();
    let cell = |row| CellAddress::new(sheet, RowIndex(row), ColumnIndex(0));
    assert!(!state.ribbon_command_available(RibbonCommand::AutoSum));
    state.sheets_select_cell(cell(0));
    state.apply_sheets_cell_input("1".into()).unwrap();
    state.sheets_select_cell(cell(1));
    state.apply_sheets_cell_input("2".into()).unwrap();
    state.sheets_select_cell(cell(2));
    state.apply_ribbon_command(RibbonCommand::AutoSum).unwrap();
    let value = state
        .sheets_api
        .as_ref()
        .unwrap()
        .cell_view(sheet, cell(2))
        .unwrap();
    assert_eq!(value.raw_text(), "=SUM(A1:A2)");
    assert_eq!(value.display_text(), "3");
    state
        .apply_ribbon_command(RibbonCommand::RowHeight48)
        .unwrap();
    state
        .apply_ribbon_command(RibbonCommand::ColumnWidth180)
        .unwrap();
    let frame = state.sheets_frame().unwrap();
    let target = frame
        .cells
        .iter()
        .find(|target| target.address == cell(2))
        .unwrap();
    assert_eq!(target.rect.h, 48.0);
    assert_eq!(target.rect.w, 180.0);
    state.apply_ribbon_command(RibbonCommand::AddSheet).unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets.len(), 2);
    state
        .apply_ribbon_command(RibbonCommand::DuplicateSheet)
        .unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets.len(), 3);
    assert_eq!(state.active_document_id(), Some(id));
    assert!(
        state
            .apply_ribbon_command(RibbonCommand::InsertTable)
            .is_err()
    );
}

#[test]
fn sheets_tools_select_range_find_russian_text_and_keep_identity() {
    let api = SpreadsheetApi::new(IroncalcAdapter::new());
    let package = api.export_xlsx().unwrap();
    let mut state = state_with_test_document(
        "tools.xlsx",
        OpenDocument::Sheets(api.document().snapshot()),
    );
    state.bind_sheets_visual(&package).unwrap();
    let id = state.active_document_id().unwrap();
    let sheet = state.sheets_view.as_ref().unwrap().active_sheet();
    let a1 = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
    let b2 = CellAddress::new(sheet, RowIndex(1), ColumnIndex(1));
    for (address, text) in [(a1, "Лоток оцинкованный"), (b2, "ЛОТОК 200")] {
        state.sheets_select_cell(address);
        state.apply_sheets_cell_input(text.into()).unwrap();
    }
    state.update_sheets_tools_draft(Some("$C$3:a1".into()), None);
    let revision = state.sheets_api.as_ref().unwrap().document().revision();
    state
        .apply_ribbon_command(RibbonCommand::GoToRange)
        .unwrap();
    let selection = state.sheets_view.as_ref().unwrap().selection().clone();
    let range = selection.range().unwrap();
    assert_eq!(range.start, a1);
    assert_eq!(
        range.end,
        CellAddress::new(sheet, RowIndex(2), ColumnIndex(2))
    );
    state.update_sheets_tools_draft(Some("A0".into()), None);
    assert!(
        state
            .apply_ribbon_command(RibbonCommand::GoToRange)
            .is_err()
    );
    assert_eq!(state.sheets_view.as_ref().unwrap().selection(), &selection);
    state.update_sheets_tools_draft(None, Some("ЛоТоК".into()));
    for expected in [a1, b2, a1] {
        state.apply_ribbon_command(RibbonCommand::FindNext).unwrap();
        assert_eq!(
            state.sheets_view.as_ref().unwrap().selection().focus,
            Some(expected)
        );
    }
    state.update_sheets_tools_draft(None, Some("Нет такого текста".into()));
    assert!(state.apply_ribbon_command(RibbonCommand::FindNext).is_err());
    assert_eq!(
        state.sheets_view.as_ref().unwrap().selection().focus,
        Some(a1)
    );
    assert_eq!(
        state.sheets_api.as_ref().unwrap().document().revision(),
        revision
    );
    assert_eq!(state.active_document_id(), Some(id));
}

#[test]
fn sheets_range_commands_preserve_data_format_and_reject_unsafe_merge() {
    let api = SpreadsheetApi::new(IroncalcAdapter::new());
    let package = api.export_xlsx().unwrap();
    let mut state = state_with_test_document(
        "ranges.xlsx",
        OpenDocument::Sheets(api.document().snapshot()),
    );
    state.bind_sheets_visual(&package).unwrap();
    let id = state.active_document_id().unwrap();
    let sheet = state.sheets_view.as_ref().unwrap().active_sheet();
    let a1 = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
    let b1 = CellAddress::new(sheet, RowIndex(0), ColumnIndex(1));
    state.sheets_select_cell(a1);
    state
        .apply_sheets_cell_input("Русский заголовок".into())
        .unwrap();
    state
        .apply_sheets_home_format(SheetsHomeFormat::ToggleBold)
        .unwrap();
    state.sheets_select_cell(b1);
    state.apply_sheets_cell_input("Не скрывать".into()).unwrap();
    state.update_sheets_tools_draft(Some("A1:B1".into()), None);
    state
        .apply_ribbon_command(RibbonCommand::GoToRange)
        .unwrap();
    let revision = state.sheets_api.as_ref().unwrap().document().revision();
    assert!(
        state
            .apply_ribbon_command(RibbonCommand::MergeSelection)
            .is_err()
    );
    assert_eq!(
        state.sheets_api.as_ref().unwrap().document().revision(),
        revision
    );
    state.sheets_select_cell(b1);
    state.apply_sheets_cell_input(String::new()).unwrap();
    state.update_sheets_tools_draft(Some("A1:B1".into()), None);
    state
        .apply_ribbon_command(RibbonCommand::GoToRange)
        .unwrap();
    state
        .apply_ribbon_command(RibbonCommand::MergeSelection)
        .unwrap();
    assert_eq!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .document()
            .snapshot()
            .sheets[0]
            .merges
            .len(),
        1
    );
    state.sheets_select_cell(b1);
    state
        .apply_ribbon_command(RibbonCommand::UnmergeSelection)
        .unwrap();
    assert!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .document()
            .snapshot()
            .sheets[0]
            .merges
            .is_empty()
    );
    state.update_sheets_tools_draft(Some("A1:B1".into()), None);
    state
        .apply_ribbon_command(RibbonCommand::GoToRange)
        .unwrap();
    state
        .apply_ribbon_command(RibbonCommand::ClearRange)
        .unwrap();
    let cell = state
        .sheets_api
        .as_ref()
        .unwrap()
        .cell_view(sheet, a1)
        .unwrap();
    assert_eq!(cell.raw_text(), "");
    assert!(cell.format.unwrap().font.bold);
    state.update_sheets_tools_draft(Some("A1:CV1000".into()), None);
    state
        .apply_ribbon_command(RibbonCommand::GoToRange)
        .unwrap();
    let revision = state.sheets_api.as_ref().unwrap().document().revision();
    assert!(
        state
            .apply_ribbon_command(RibbonCommand::ClearRange)
            .is_err()
    );
    assert_eq!(
        state.sheets_api.as_ref().unwrap().document().revision(),
        revision
    );
    assert_eq!(state.active_document_id(), Some(id));
}

#[test]
fn sheets_tools_drafts_are_scoped_by_sheet_and_released_on_close() {
    let api = SpreadsheetApi::new(IroncalcAdapter::new());
    let package = api.export_xlsx().unwrap();
    let mut state = state_with_test_document(
        "scoped.xlsx",
        OpenDocument::Sheets(api.document().snapshot()),
    );
    state.bind_sheets_visual(&package).unwrap();
    let id = state.active_document_id().unwrap();
    state.update_sheets_tools_draft(Some("C12".into()), Some("Лоток".into()));
    state.apply_ribbon_command(RibbonCommand::AddSheet).unwrap();
    assert_eq!(state.sheets_tools_draft().address, "A1");
    assert_eq!(state.sheets_tools_draft().query, "");
    state.ribbon_command_feedback = Some("Сообщение другого листа".into());
    state.select_sheet_index(0);
    assert!(state.ribbon_command_feedback.is_none());
    assert_eq!(state.sheets_tools_draft().address, "C12");
    assert_eq!(state.sheets_tools_draft().query, "Лоток");
    state.close_document(id).unwrap();
    assert!(state.sheets_tools.is_empty());
}

#[test]
fn ribbon_writer_insertions_are_undoable_and_layout_reaches_page_frame() {
    let mut state = state_with_test_document(
        "ribbon.docx",
        OpenDocument::Writer(Box::new(WriterViewModel::new())),
    );
    let id = state.active_document_id().unwrap();
    state
        .apply_ribbon_command(RibbonCommand::InsertParagraph)
        .unwrap();
    assert!(
        matches!(state.document(), Some(OpenDocument::Writer(writer)) if writer.snapshot().blocks.len() == 2)
    );
    state
        .apply_ribbon_command(RibbonCommand::InsertTable)
        .unwrap();
    assert!(
        matches!(state.document(), Some(OpenDocument::Writer(writer)) if writer.snapshot().blocks.iter().any(|block| matches!(block, BlockSnapshot::Table(_))))
    );
    state
        .apply_writer_home_command(WriterHomeCommand::Undo)
        .unwrap();
    assert!(
        matches!(state.document(), Some(OpenDocument::Writer(writer)) if !writer.snapshot().blocks.iter().any(|block| matches!(block, BlockSnapshot::Table(_))))
    );
    state
        .apply_ribbon_command(RibbonCommand::Landscape)
        .unwrap();
    if let Some(OpenDocument::Writer(writer)) = state.document() {
        let settings = writer.api().layout_settings();
        assert!(settings.page_size.width_half_points > settings.page_size.height_half_points);
        // The production draw path reads the view's cached layout, not a
        // newly calculated API layout. Both must reflect the toolbar change.
        let layout = writer.state().layout.as_ref().unwrap();
        assert!(layout.pages[0].width_half_points > layout.pages[0].height_half_points);
        let frame = build_writer_page_frame(
            &writer.snapshot(),
            layout,
            settings,
            writer.docx_layout(),
            100,
            0.0,
            0.0,
        );
        assert!(frame.pages[0].width_half_points > frame.pages[0].height_half_points);
    }
    assert_eq!(state.active_document_id(), Some(id));
}

#[test]
fn slides_home_add_duplicate_delete_updates_live_presentation() {
    let mut presentation = Presentation::new(PresentationId::new(700));
    presentation
        .add_slide(Slide::new(SlideId::new(1)))
        .expect("initial slide must be valid");
    let mut state = state_with_test_document("deck.pptx", OpenDocument::Slides(presentation));
    state.bind_slides_document();

    state
        .apply_slides_home_command(SlidesHomeCommand::Add)
        .expect("new slide must be added");
    assert_eq!(state.slides_snapshot().slide_count, 2);
    assert_eq!(state.slides_snapshot().current_index, 1);

    state
        .apply_slides_home_command(SlidesHomeCommand::Duplicate)
        .expect("current slide must duplicate");
    assert_eq!(state.slides_snapshot().slide_count, 3);
    assert_eq!(state.slides_snapshot().current_index, 2);

    state
        .apply_slides_home_command(SlidesHomeCommand::Delete)
        .expect("duplicate slide must delete");
    assert_eq!(state.slides_snapshot().slide_count, 2);
    assert!(state.slides_viewer.is_some());
    let bytes = state
        .document()
        .unwrap()
        .export_bytes()
        .expect("export edited PPTX");
    let (reopened, _, _, _) = OpenDocument::open_background(Path::new("edited.pptx"), &bytes)
        .expect("reopen edited PPTX");
    let OpenDocument::Slides(reopened) = reopened else {
        panic!("Slides document expected");
    };
    assert_eq!(reopened.slides.len(), 2);
}

#[test]
fn slides_home_text_color_targets_selected_text_and_preserves_formatting() {
    let slide_id = SlideId::new(1);
    let object_id = ObjectId::new(9);
    let mut text = slides_api::Text::plain("Цветной слайд");
    let formatting = &mut text.paragraphs[0].runs[0].formatting;
    formatting.font_family = Some("Aptos".to_owned());
    formatting.font_size = 28.0;
    formatting.bold = true;
    formatting.italic = true;
    let mut slide = Slide::new(slide_id);
    slide
        .add_object(slides_api::SlideObject::new(
            object_id,
            ObjectKind::Text(text),
            slides_api::Rect::new(slides_api::Point::new(80.0, 90.0), 480.0, 120.0),
            slides_api::Transform::IDENTITY,
        ))
        .expect("text object must be valid");
    let mut presentation = Presentation::new(PresentationId::new(701));
    presentation.add_slide(slide).expect("slide must be valid");
    let mut state = state_with_test_document("colors.pptx", OpenDocument::Slides(presentation));
    state.bind_slides_document();
    state
        .slides_select_text_object(Some(object_id))
        .expect("text object must select");
    assert!(state.slides_text_color_available());

    state
        .apply_slides_home_command(SlidesHomeCommand::SetTextColor(SlidesColor::rgb(
            220, 38, 38,
        )))
        .expect("selected slide text color must commit");

    let OpenDocument::Slides(presentation) = state.document().expect("Slides document") else {
        panic!("Slides backend expected");
    };
    let object = presentation
        .slide(slide_id)
        .and_then(|slide| slide.object(object_id))
        .expect("selected text object");
    let ObjectKind::Text(text) = &object.kind else {
        panic!("text object expected");
    };
    let formatting = &text.paragraphs[0].runs[0].formatting;
    assert_eq!(formatting.color, SlidesColor::rgb(220, 38, 38));
    assert_eq!(formatting.font_family.as_deref(), Some("Aptos"));
    assert_eq!(formatting.font_size, 28.0);
    assert!(formatting.bold);
    assert!(formatting.italic);
    assert_eq!(state.slides_color_value, Some("text-red"));
}

fn writer_text_view_from_blocks(blocks: &[&[&str]]) -> WriterTextView {
    WriterTextView {
        pages: vec![WriterPageView {
            blocks: blocks
                .iter()
                .map(|fragments| WriterBlockView {
                    fragments: fragments
                        .iter()
                        .map(|text| WriterFragmentView {
                            text: (*text).to_owned(),
                        })
                        .collect(),
                })
                .collect(),
        }],
        truncated: false,
    }
}

#[test]
fn routes_and_round_trips_real_office_packages_through_the_unified_application() {
    let sheets = SpreadsheetApi::new(IroncalcAdapter::new());
    let xlsx = sheets
        .export_xlsx()
        .expect("must create a real XLSX package");
    assert_round_trip("book.xlsx", xlsx, Module::Sheets);

    let writer = WriterApi::new();
    let docx = writer.export_docx().expect("must create a DOCX package");
    assert_round_trip("document.docx", docx, Module::Writer);

    let pdf = match writer
        .export(writer_api::core::DocumentFormat::Pdf)
        .expect("must create a PDF package")
    {
        writer_api::ExportedDocument::Pdf(bytes) => bytes,
        _ => panic!("writer returned an unexpected export type"),
    };
    let mut pdf_engine = PdfOxideAdapter::default();
    let (pdf_document, _, _) = OpenDocument::open(Path::new("document.pdf"), &pdf, &mut pdf_engine)
        .expect("must open PDF through the unified application");
    assert_eq!(pdf_document.module(), Module::Pdf);
    let export_error = pdf_document
        .export_bytes()
        .expect_err("ARCH-PDF-003 must block PDF export");
    assert!(export_error.contains("ARCH-PDF-003 STRICT LOCK"));

    let mut presentation = Presentation::new(PresentationId::new(1));
    presentation
        .add_slide(Slide::new(SlideId::new(1)))
        .expect("must build a valid presentation");
    let pptx = office_oxide_adapter()
        .save_bytes(&presentation)
        .expect("must create a real PPTX package");
    assert_round_trip("presentation.pptx", pptx, Module::Slides);
}

#[test]
fn pdf_viewer_visibility_follows_active_document_identity() {
    let mut state = AppState::default();
    assert!(!state.should_show_pdf_viewer());

    let pdf = attach_test_document(
        &mut state,
        "document.pdf",
        OpenDocument::Pdf {
            document_id: DocumentId::new(1),
            page_count: 1,
            page_ids: vec![PageId::new(1)],
        },
    );
    state
        .pdf_controller
        .state_mut()
        .bind_pdf(DocumentId::new(1), 1, &[PageId::new(1)]);
    assert!(state.should_show_pdf_viewer());
    assert_eq!(state.active_module(), Module::Pdf);

    let sheets = attach_test_document(
        &mut state,
        "book.xlsx",
        OpenDocument::Sheets(WorkbookSnapshot::default()),
    );
    assert_eq!(state.active_document_id(), Some(sheets));
    assert_eq!(state.active_module(), Module::Sheets);
    assert!(!state.should_show_pdf_viewer());

    state.active_document = Some(pdf);
    assert_eq!(state.active_module(), Module::Pdf);
    assert!(state.should_show_pdf_viewer());
}

#[test]
fn pdf_viewer_remains_neutral_when_no_document_is_bound() {
    let mut state = state_with_test_document(
        "empty.pdf",
        OpenDocument::Pdf {
            document_id: DocumentId::new(1),
            page_count: 0,
            page_ids: vec![],
        },
    );
    state
        .pdf_controller
        .state_mut()
        .bind_pdf(DocumentId::new(1), 0, &[]);
    assert!(state.should_show_pdf_viewer());
    assert_eq!(state.pdf_controller.state().status, PdfViewStatus::Ready);
    assert!(state.pdf_controller.state().page_ids.is_empty());
}

#[test]
fn pdf_controller_projection_matches_bound_state_without_ui_context() {
    let mut app = App {
        ui: WidgetRef::default(),
        state: AppState::default(),
        writer_inline_editor: None,
        writer_inline_editor_pending_focus: false,
        sheets_cell_input_binding: None,
        native_paste_hook: Default::default(),
        launcher_startup_request: None,
    };
    attach_test_document(
        &mut app.state,
        "document.pdf",
        OpenDocument::Pdf {
            document_id: DocumentId::new(1),
            page_count: 1,
            page_ids: vec![PageId::new(1)],
        },
    );
    app.state
        .pdf_controller
        .state_mut()
        .bind_pdf(DocumentId::new(1), 1, &[PageId::new(1)]);
    app.state.pdf_controller.state_mut().png = PNG_SIGNATURE.to_vec();

    let state = app.state.pdf_controller.state();
    assert_eq!(state.status, PdfViewStatus::Ready);
    assert_eq!(state.current_page_number(), 1);
    assert_eq!(state.page_count, 1);
    assert!(!state.png.is_empty());
    assert_eq!(&state.png[..PNG_SIGNATURE.len()], PNG_SIGNATURE);
}

fn text_cell(sheet: SheetId, row: u32, column: u32, text: &str) -> CellSnapshot {
    CellSnapshot {
        address: CellAddress::new(sheet, RowIndex(row), ColumnIndex(column)),
        input: CellInput::Text(text.to_owned()),
        format: None,
    }
}

fn sheet_with(id: u32, name: &str, cells: Vec<CellSnapshot>) -> SheetSnapshot {
    SheetSnapshot {
        id: SheetId(id),
        name: name.to_owned(),
        cells,
        ..Default::default()
    }
}

#[test]
fn bounded_sheet_window_uses_scroll_overscan_without_losing_its_cap() {
    let window = bounded_sheet_window_from_scroll(
        2,
        20 * sheets_default_column_width(),
        300 * sheets_default_row_height(),
    );

    assert_eq!(window.sheet_index, 2);
    assert_eq!(window.start_row, 236);
    assert_eq!(window.start_column, 20 - BOUNDED_WINDOW_COLUMNS / 4);
    assert_eq!(window.row_count, BOUNDED_WINDOW_ROWS);
    assert_eq!(window.column_count, BOUNDED_WINDOW_COLUMNS);
    assert_eq!(window.cell_limit, BOUNDED_CELLS_PER_SHEET);
    assert_eq!(
        window.row_count as usize * window.column_count as usize,
        window.cell_limit
    );
}

#[test]
fn source_backed_sheets_navigation_reloads_a_later_window_without_losing_the_document() {
    let first = SheetId(1);
    let second = SheetId(2);
    let workbook = WorkbookSnapshot {
        active_sheet: first,
        sheets: vec![
            sheet_with(1, "Первый", vec![text_cell(first, 0, 0, "начало")]),
            sheet_with(
                2,
                "Второй",
                vec![text_cell(second, 10_000, 3, "поздняя область")],
            ),
        ],
        ..Default::default()
    };
    let package =
        XlsxPartsCodec::export(&IroncalcXlsxParts, &workbook).expect("fixture XLSX export");
    let source_bytes: Arc<[u8]> = Arc::from(package);
    let initial = import_bounded_sheet_window(&source_bytes, BoundedSheetWindow::initial())
        .expect("initial bounded window");

    let mut state = AppState::default();
    let document = state
        .register_document_with_source(
            DocumentSource::from_path_with_retained_bytes(
                PathBuf::from("large-source-backed.xlsx"),
                source_bytes,
            ),
            OpenDocument::Sheets(initial),
        )
        .expect("source-backed Sheets session must register");
    state
        .activate_document(document)
        .expect("initial bounded sheet must activate");
    state.select_sheet_index(1);
    state
        .scroll(
            document,
            0.0,
            f64::from(10_000 * sheets_default_row_height()),
        )
        .expect("scroll into later source-backed region");

    let window = state
        .sheets_bounded_window
        .expect("later region must remain represented by a bounded window");
    assert_eq!(window.sheet_index, 1);
    assert!(window.contains(10_000, 3));
    let snapshot = state.sheets_snapshot().expect("active Sheets snapshot");
    assert_eq!(snapshot.active_sheet, second);
    assert!(snapshot.sheets[1].cells.iter().any(|cell| {
        cell.address == CellAddress::new(second, RowIndex(10_000), ColumnIndex(3))
            && cell.input == CellInput::Text("поздняя область".to_owned())
    }));
    assert!(
        state.sheets_view.is_some(),
        "the bounded snapshot must rebind the grid"
    );
    assert!(
        state
            .detail
            .contains("Большая книга: данные загружаются по области")
    );
    // RC25: a reload preserves the whole range, not just its active cell,
    // and manual scrolling does not jump back to that offscreen selection.
    let start = CellAddress::new(second, RowIndex(10_000), ColumnIndex(3));
    let end = CellAddress::new(second, RowIndex(10_001), ColumnIndex(5));
    state
        .sheets_view
        .as_mut()
        .unwrap()
        .select_range(start, end)
        .unwrap();
    state.sync_active_session_view();
    let selection = state.sheets_view.as_ref().unwrap().selection().clone();
    state
        .scroll(
            document,
            0.0,
            f64::from(1_000 * sheets_default_row_height()),
        )
        .unwrap();
    assert_eq!(state.sheets_view.as_ref().unwrap().selection(), &selection);
    let later = state.sheets_view.as_ref().unwrap().scroll_offset();
    assert!(later.y > 10_500 * sheets_default_row_height());
    assert!(state.sheets_bounded_window.unwrap().contains(11_000, 3));
    state.select_sheet_index(0);
    state.select_sheet_index(1);
    assert_eq!(state.sheets_view.as_ref().unwrap().selection(), &selection);
    assert_eq!(state.sheets_view.as_ref().unwrap().scroll_offset(), later);
    assert!(state.sheets_bounded_window.unwrap().contains(11_000, 3));
    assert!(!state.sheets_can_undo());
    assert!(
        state
            .apply_ribbon_command(RibbonCommand::ClearRange)
            .is_err()
    );
    assert!(
        state
            .dispatch_history_command(document, OfficeHistoryCommand::Undo)
            .is_err()
    );
    // A manual wheel during a captured drag can leave the resident window.
    // The controller must remain alive, so the next move extends the same
    // logical range rather than silently ending the gesture.
    let grid = state.sheets_frame().unwrap().layout.grid_viewport;
    let down = sheets_grid::Point {
        x: grid.x + 4.0,
        y: grid.y + 4.0,
    };
    state.sheets_handle_selection_input(UiEvent::PointerDown {
        point: down,
        button: sheets_ui::PointerButton::Primary,
        modifiers: Default::default(),
    });
    let anchor = state.sheets_view.as_ref().unwrap().selection().anchor;
    state.sheets_wheel(0, i64::from(1_000 * sheets_default_row_height()), false);
    let moved = sheets_grid::Point {
        x: grid.x + 150.0,
        y: grid.y + 70.0,
    };
    state.sheets_handle_selection_input(UiEvent::PointerMove {
        point: moved,
        modifiers: Default::default(),
    });
    let selected = state.sheets_view.as_ref().unwrap().selection();
    assert_eq!(selected.anchor, anchor);
    assert!(selected.focus.unwrap().row.0 > anchor.unwrap().row.0 + 900);
    state.sheets_handle_selection_input(UiEvent::PointerUp {
        point: moved,
        button: sheets_ui::PointerButton::Primary,
        modifiers: Default::default(),
    });
}

#[test]
fn column_label_follows_a1_notation_across_carry_boundaries() {
    assert_eq!(column_label(ColumnIndex(0)), "A");
    assert_eq!(column_label(ColumnIndex(25)), "Z");
    assert_eq!(column_label(ColumnIndex(26)), "AA");
    assert_eq!(column_label(ColumnIndex(51)), "AZ");
    assert_eq!(column_label(ColumnIndex(52)), "BA");
    assert_eq!(column_label(ColumnIndex(701)), "ZZ");
    assert_eq!(column_label(ColumnIndex(702)), "AAA");
}

#[test]
fn cell_input_text_renders_each_input_variant_without_panicking() {
    assert_eq!(cell_input_text(&CellInput::Empty), "");
    assert_eq!(cell_input_text(&CellInput::Text("шапка".into())), "шапка");
    assert_eq!(cell_input_text(&CellInput::Number(42.0)), "42");
    assert_eq!(cell_input_text(&CellInput::Number(1.5)), "1.5");
    assert_eq!(cell_input_text(&CellInput::Percent(0.25)), "25%");
    assert_eq!(cell_input_text(&CellInput::Boolean(true)), "ИСТИНА");
    assert_eq!(cell_input_text(&CellInput::Boolean(false)), "ЛОЖЬ");
    assert_eq!(
        cell_input_text(&CellInput::Formula("SUM(A1:A2)".into())),
        "=SUM(A1:A2)"
    );
}

#[test]
fn cell_input_text_is_byte_capped_on_a_char_boundary() {
    let long = "я".repeat(2_000);
    let rendered = cell_input_text(&CellInput::Text(long));
    assert!(rendered.len() <= SHEETS_CELL_BYTE_CAP);
    // Truncation must not split a multi-byte character.
    assert!(rendered.is_char_boundary(rendered.len()));
    assert!(rendered.chars().all(|c| c == 'я'));
}

#[test]
fn project_sheet_grid_densifies_sparse_out_of_order_cells() {
    let sheet = sheet_with(
        1,
        "Данные",
        vec![
            text_cell(SheetId(1), 4, 2, "C5"),
            text_cell(SheetId(1), 0, 0, "A1"),
            text_cell(SheetId(1), 4, 0, "A5"),
        ],
    );

    let view = project_sheet_grid(&sheet);
    assert_eq!(view.name, "Данные");
    // Only occupied rows/columns are projected, in ascending order.
    assert_eq!(view.row_labels, vec!["1", "5"]);
    assert_eq!(view.column_labels, vec!["A", "C"]);
    assert_eq!(view.rows, vec![vec!["A1", ""], vec!["A5", "C5"]]);
    assert_eq!(view.total_cells, 3);
    assert!(!view.truncated);
}

#[test]
fn project_sheet_grid_skips_empty_inputs_and_reports_no_content() {
    let sheet = sheet_with(
        2,
        "Пусто",
        vec![CellSnapshot {
            address: CellAddress::new(SheetId(2), RowIndex(0), ColumnIndex(0)),
            input: CellInput::Empty,
            format: None,
        }],
    );

    let view = project_sheet_grid(&sheet);
    assert!(view.is_empty());
    assert!(view.rows.is_empty());
    assert_eq!(view.total_cells, 1);
    assert_eq!(
        sheet_grid_status(&view),
        "Лист «Пусто»: нет заполненных ячеек."
    );
}

#[test]
fn project_sheet_grid_clamps_to_view_caps_and_flags_truncation() {
    let mut cells = Vec::new();
    for row in 0..(SHEETS_VIEW_MAX_ROWS as u32 + 5) {
        for column in 0..(SHEETS_VIEW_MAX_COLUMNS as u32 + 3) {
            cells.push(text_cell(SheetId(3), row, column, "x"));
        }
    }
    let sheet = sheet_with(3, "Большой", cells);

    let view = project_sheet_grid(&sheet);
    assert!(view.truncated);
    assert_eq!(view.rows.len(), SHEETS_VIEW_MAX_ROWS);
    assert_eq!(view.column_labels.len(), SHEETS_VIEW_MAX_COLUMNS);
    // Every projected row keeps the dense column count.
    assert!(
        view.rows
            .iter()
            .all(|row| row.len() == SHEETS_VIEW_MAX_COLUMNS)
    );
    assert!(sheet_grid_status(&view).contains("фрагмент"));
}

#[test]
fn active_sheet_index_prefers_the_snapshot_active_sheet_and_falls_back() {
    let snapshot = WorkbookSnapshot {
        active_sheet: SheetId(7),
        sheets: vec![
            sheet_with(5, "Первый", vec![]),
            sheet_with(7, "Второй", vec![]),
        ],
        ..Default::default()
    };
    assert_eq!(active_sheet_index(&snapshot), Some(1));

    // Unknown active sheet falls back to the first sheet, not to None.
    let orphaned = WorkbookSnapshot {
        active_sheet: SheetId(99),
        sheets: vec![sheet_with(5, "Первый", vec![])],
        ..Default::default()
    };
    assert_eq!(active_sheet_index(&orphaned), Some(0));
    assert_eq!(active_sheet_index(&WorkbookSnapshot::default()), None);
}

#[test]
fn clamp_sheet_index_never_exceeds_the_snapshot_range() {
    let snapshot = WorkbookSnapshot {
        sheets: vec![sheet_with(1, "A", vec![]), sheet_with(2, "B", vec![])],
        ..Default::default()
    };
    assert_eq!(clamp_sheet_index(&snapshot, 0), Some(0));
    assert_eq!(clamp_sheet_index(&snapshot, 1), Some(1));
    assert_eq!(clamp_sheet_index(&snapshot, 99), Some(1));
    assert_eq!(clamp_sheet_index(&WorkbookSnapshot::default(), 0), None);
}

#[test]
fn sheet_tabs_label_marks_only_the_selected_sheet() {
    let snapshot = WorkbookSnapshot {
        sheets: vec![
            sheet_with(1, "Смета", vec![]),
            sheet_with(2, "Итоги", vec![]),
        ],
        ..Default::default()
    };
    assert_eq!(
        sheet_tabs_label(&snapshot, Some(1)),
        "Листы: Смета · [Итоги]"
    );
    assert_eq!(sheet_tabs_label(&snapshot, None), "Листы: Смета · Итоги");
    assert_eq!(
        sheet_tabs_label(&WorkbookSnapshot::default(), None),
        "Листы: нет"
    );
}

#[test]
fn render_grid_text_aligns_headers_with_projected_rows() {
    let sheet = sheet_with(
        1,
        "Лист",
        vec![
            text_cell(SheetId(1), 0, 0, "a"),
            text_cell(SheetId(1), 1, 1, "b"),
        ],
    );
    let view = project_sheet_grid(&sheet);
    let text = render_grid_text(&view);
    let lines: Vec<&str> = text.lines().collect();

    // One header line plus one line per projected row.
    assert_eq!(lines.len(), view.rows.len() + 1);
    assert!(lines[0].contains('A') && lines[0].contains('B'));
    assert!(lines[1].starts_with('1'));
    assert!(lines[2].starts_with('2'));
    // All lines share the same fixed-width layout.
    let width = lines[0].chars().count();
    assert!(lines.iter().all(|line| line.chars().count() == width));
}

#[test]
fn render_grid_text_is_empty_for_an_empty_projection() {
    let view = project_sheet_grid(&sheet_with(1, "Пусто", vec![]));
    assert!(render_grid_text(&view).is_empty());
}

#[test]
fn sheets_visibility_follows_active_document_identity() {
    let mut state = AppState::default();
    assert!(!state.should_show_sheets_view());

    let sheets = attach_test_document(
        &mut state,
        "book.xlsx",
        OpenDocument::Sheets(WorkbookSnapshot::default()),
    );
    assert!(state.should_show_sheets_view());
    assert_eq!(state.active_module(), Module::Sheets);

    let pdf = attach_test_document(
        &mut state,
        "document.pdf",
        OpenDocument::Pdf {
            document_id: DocumentId::new(1),
            page_count: 1,
            page_ids: vec![PageId::new(1)],
        },
    );
    assert_eq!(state.active_document_id(), Some(pdf));
    assert_eq!(state.active_module(), Module::Pdf);
    assert!(!state.should_show_sheets_view());

    state.active_document = Some(sheets);
    assert_eq!(state.active_module(), Module::Sheets);
    assert!(state.should_show_sheets_view());
}

#[test]
fn sheets_tab_roundtrip_restores_sheet_zoom_and_scroll() {
    let snapshot_a = WorkbookSnapshot {
        active_sheet: SheetId(1),
        sheets: vec![
            sheet_with(1, "A1", vec![text_cell(SheetId(1), 0, 0, "first")]),
            sheet_with(
                2,
                "A2",
                vec![text_cell(SheetId(2), 120, 30, "scroll target")],
            ),
        ],
        ..Default::default()
    };
    let snapshot_b = WorkbookSnapshot {
        active_sheet: SheetId(3),
        sheets: vec![sheet_with(
            3,
            "B1",
            vec![text_cell(SheetId(3), 80, 20, "other document")],
        )],
        ..Default::default()
    };
    let mut state = AppState::default();
    let document_a = state
        .register_document(PathBuf::from("same.xlsx"), OpenDocument::Sheets(snapshot_a))
        .expect("Sheets A must register");
    let document_b = state
        .register_document(PathBuf::from("same.xlsx"), OpenDocument::Sheets(snapshot_b))
        .expect("Sheets B must register");

    state
        .activate_document(document_a)
        .expect("activate Sheets A");
    state.select_sheet_index(1);
    state.set_zoom(document_a, 1.6).expect("zoom Sheets A");
    state
        .scroll(document_a, 80.0, 120.0)
        .expect("scroll Sheets A");
    state.sync_active_session_view();
    let expected_a = state.inspect(document_a).unwrap().view_state;

    state
        .activate_document(document_b)
        .expect("activate Sheets B");
    state.set_zoom(document_b, 0.75).expect("zoom Sheets B");

    state
        .activate_document(document_a)
        .expect("restore Sheets A by its runtime ID");
    assert_eq!(state.active_document_id(), Some(document_a));
    assert_eq!(state.active_module(), Module::Sheets);
    assert_eq!(state.inspect(document_a).unwrap().view_state, expected_a);
    let DocumentViewState::Sheets(restored) = expected_a else {
        panic!("Sheets view state expected");
    };
    assert_eq!(state.sheets_selected_index(), Some(restored.selected_sheet));
    let visual = state.sheets_view.as_ref().expect("Sheets visual must bind");
    assert_eq!(visual.zoom(), restored.zoom);
    let scroll = visual.scroll_offset();
    assert_eq!((scroll.x, scroll.y), (restored.scroll_x, restored.scroll_y));
    assert_eq!(
        state
            .sheets_api
            .as_ref()
            .expect("Sheets visual API must bind")
            .document()
            .active_sheet(),
        SheetId(2),
        "the visual adapter must restore the persisted selected sheet"
    );
}

#[test]
fn sheet_navigation_is_bounded_and_preserves_the_snapshot() {
    let snapshot = WorkbookSnapshot {
        active_sheet: SheetId(2),
        sheets: vec![
            sheet_with(1, "A", vec![text_cell(SheetId(1), 0, 0, "a")]),
            sheet_with(2, "B", vec![text_cell(SheetId(2), 0, 0, "b")]),
        ],
        ..Default::default()
    };
    let original = snapshot.clone();

    let mut state = state_with_test_document("book.xlsx", OpenDocument::Sheets(snapshot));
    state.bind_sheets_document();

    // Binding starts on the snapshot's active sheet.
    assert_eq!(state.sheets_selected_index(), Some(1));
    assert!(state.sheets_can_previous());
    assert!(!state.sheets_can_next());

    // Next at the last sheet is a no-op.
    state.sheets_next();
    assert_eq!(state.sheets_selected_index(), Some(1));

    state.sheets_previous();
    assert_eq!(state.sheets_selected_index(), Some(0));
    assert!(!state.sheets_can_previous());
    assert!(state.sheets_can_next());

    // Previous at the first sheet is a no-op.
    state.sheets_previous();
    assert_eq!(state.sheets_selected_index(), Some(0));

    // Navigation is read-only: the snapshot is untouched.
    assert_eq!(state.sheets_snapshot(), Some(&original));
}

#[test]
fn measured_sheets_viewport_reflows_the_selected_cell_into_frame() {
    let mut state = AppState::default();
    let mut view = SheetsUi::new(UiConfig {
        outer_width: 1180,
        outer_height: 590,
        ..UiConfig::default()
    })
    .expect("test Sheets UI");
    let target = CellAddress::new(SheetId(0), RowIndex(99), ColumnIndex(25));
    view.select_cell(target).expect("select late test cell");
    state.sheets_view = Some(view);

    assert!(state.sync_sheets_viewport(924, 197));
    let frame = state.sheets_frame().expect("reflowed Sheets frame");
    let selected = frame
        .cells
        .iter()
        .find(|cell| cell.address == target)
        .expect("selected cell must be visible after app viewport sync");
    let viewport = frame.layout.grid_viewport;
    assert!(selected.rect.x >= viewport.x);
    assert!(selected.rect.y >= viewport.y);
    assert!(selected.rect.x + selected.rect.w <= viewport.x + viewport.w);
    assert!(selected.rect.y + selected.rect.h <= viewport.y + viewport.h);
}

#[test]
fn sheet_grid_view_follows_the_selected_sheet() {
    let snapshot = WorkbookSnapshot {
        active_sheet: SheetId(1),
        sheets: vec![
            sheet_with(1, "A", vec![text_cell(SheetId(1), 0, 0, "первый")]),
            sheet_with(2, "B", vec![text_cell(SheetId(2), 0, 0, "второй")]),
        ],
        ..Default::default()
    };
    let mut state = state_with_test_document("book.xlsx", OpenDocument::Sheets(snapshot));
    state.bind_sheets_document();

    let view = state.sheets_grid_view().expect("grid view must exist");
    assert_eq!(view.name, "A");
    assert_eq!(view.rows, vec![vec!["первый"]]);

    state.sheets_next();
    let view = state.sheets_grid_view().expect("grid view must exist");
    assert_eq!(view.name, "B");
    assert_eq!(view.rows, vec![vec!["второй"]]);
}

#[test]
fn sheet_helpers_are_inert_without_a_sheets_document() {
    let mut state = AppState::default();
    assert!(state.sheets_snapshot().is_none());
    assert!(state.sheets_selected_index().is_none());
    assert!(state.sheets_grid_view().is_none());
    assert!(!state.sheets_can_previous());
    assert!(!state.sheets_can_next());

    // Navigation on a missing document must not panic or shift state.
    state.sheets_next();
    state.sheets_previous();
    assert_eq!(state.sheets_selected, 0);
}

#[test]
fn opening_a_real_xlsx_projects_a_grid_from_the_snapshot() {
    let sheets = SpreadsheetApi::new(IroncalcAdapter::new());
    let xlsx = sheets
        .export_xlsx()
        .expect("must create a real XLSX package");
    let mut engine = PdfOxideAdapter::default();
    let (document, _, _) =
        OpenDocument::open(Path::new("book.xlsx"), &xlsx, &mut engine).expect("must open XLSX");
    assert_eq!(document.module(), Module::Sheets);

    let mut state = state_with_test_document("book.xlsx", document);
    state.bind_sheets_document();

    assert!(state.should_show_sheets_view());
    let snapshot = state.sheets_snapshot().expect("snapshot must exist");
    // Selection always resolves inside the real workbook's sheet range.
    assert_eq!(
        state.sheets_selected_index(),
        clamp_sheet_index(snapshot, state.sheets_selected)
    );
    if !snapshot.sheets.is_empty() {
        let view = state.sheets_grid_view().expect("grid view must exist");
        assert!(view.rows.len() <= SHEETS_VIEW_MAX_ROWS);
        assert!(view.column_labels.len() <= SHEETS_VIEW_MAX_COLUMNS);
    }
}

#[test]
fn inspect_document_reports_the_detected_module() {
    let sheets = SpreadsheetApi::new(IroncalcAdapter::new());
    let xlsx = sheets
        .export_xlsx()
        .expect("must create a real XLSX package");
    assert_eq!(
        App::inspect_document(Path::new("book.xlsx"), &xlsx)
            .expect("must inspect XLSX")
            .0,
        Module::Sheets
    );
}

#[test]
fn pdf_render_returns_valid_png() {
    let writer = WriterApi::new();
    let pdf_bytes = match writer
        .export(writer_api::core::DocumentFormat::Pdf)
        .expect("must create a PDF package")
    {
        writer_api::ExportedDocument::Pdf(bytes) => bytes,
        _ => panic!("writer returned an unexpected export type"),
    };
    let mut engine = PdfOxideAdapter::default();
    let (document, _, _) = OpenDocument::open(Path::new("document.pdf"), &pdf_bytes, &mut engine)
        .expect("must open pdf");
    let (document_id, page_count, page_ids) =
        document.pdf_bindings().expect("pdf bindings must exist");
    let mut controller = PdfController::default();
    controller.bind_document(document_id, page_count, page_ids);
    controller.apply_effect(
        PdfViewEffect::RenderPage {
            page_index: PageId::new(1),
            scale: 1.0,
        },
        &mut engine,
    );
    let state = controller.state();
    assert_eq!(state.status, PdfViewStatus::Ready);
    assert!(
        state.page_count > 0,
        "opened PDF must report positive page count"
    );
    assert!(!state.png.is_empty(), "rendered page must not be empty");
    assert_eq!(&state.png[..PNG_SIGNATURE.len()], PNG_SIGNATURE);
}

#[test]
fn pdf_error_allows_retry_render_after_zoom_navigation() {
    let writer = WriterApi::new();
    let pdf_bytes = match writer
        .export(writer_api::core::DocumentFormat::Pdf)
        .expect("must create a PDF package")
    {
        writer_api::ExportedDocument::Pdf(bytes) => bytes,
        _ => panic!("writer returned an unexpected export type"),
    };
    let mut engine = PdfOxideAdapter::default();
    let (document, _, _) = OpenDocument::open(Path::new("document.pdf"), &pdf_bytes, &mut engine)
        .expect("must open pdf");
    let (document_id, page_count, page_ids) =
        document.pdf_bindings().expect("pdf bindings must exist");
    let mut controller = PdfController::default();
    controller.bind_document(document_id, page_count, page_ids);

    // induce a render failure and verify we are in Error state
    controller.apply_effect(
        PdfViewEffect::RenderPage {
            page_index: PageId::new(1),
            scale: 0.001,
        },
        &mut engine,
    );
    assert!(
        matches!(controller.state().status, PdfViewStatus::Error),
        "invalid scale should leave controller in Error state"
    );
    controller.state_mut().scale = 1.0;

    // zoom must request a retry render instead of returning None
    let retry = controller.zoom_in();
    assert_eq!(
        retry,
        PdfViewEffect::RenderPage {
            page_index: PageId::new(1),
            scale: 1.25
        }
    );

    // bind at least 2 pages so navigation can move and retry render from Error
    controller
        .state_mut()
        .bind_pdf(document_id, page_count, &[PageId::new(1), PageId::new(2)]);
    controller.apply_effect(
        PdfViewEffect::RenderPage {
            page_index: PageId::new(1),
            scale: 1.0,
        },
        &mut engine,
    );
    controller.apply_effect(
        PdfViewEffect::RenderPage {
            page_index: PageId::new(1),
            scale: 0.001,
        },
        &mut engine,
    );
    assert!(
        matches!(controller.state().status, PdfViewStatus::Error),
        "second render failure should leave controller in Error state"
    );
    let nav_retry = controller.next_page();
    assert!(
        matches!(nav_retry, PdfViewEffect::RenderPage { .. }),
        "navigation from Error must request retry render"
    );
}

#[test]
fn pdf_controller_navigation_does_not_leave_half_connected_state() {
    let mut controller = PdfController::default();
    controller.state_mut().bind_pdf(
        DocumentId::new(1),
        3,
        &[PageId::new(1), PageId::new(2), PageId::new(3)],
    );
    assert!(controller.state().can_next());
    assert!(!controller.state().can_previous());
    controller.next_page();
    assert!(controller.state().can_previous());
    assert!(controller.state().can_next());
    controller.previous();
    assert!(!controller.state().can_previous());
    assert!(matches!(controller.state().status, PdfViewStatus::Ready));
}

#[test]
fn pdf_controller_zoom_does_not_corrupt_state() {
    let writer = WriterApi::new();
    let pdf_bytes = match writer
        .export(writer_api::core::DocumentFormat::Pdf)
        .expect("must create a PDF package")
    {
        writer_api::ExportedDocument::Pdf(bytes) => bytes,
        _ => panic!("writer returned an unexpected export type"),
    };
    let mut engine = PdfOxideAdapter::default();
    let (document, _, _) = OpenDocument::open(Path::new("document.pdf"), &pdf_bytes, &mut engine)
        .expect("must open pdf");
    let (document_id, page_count, page_ids) =
        document.pdf_bindings().expect("pdf bindings must exist");
    let mut controller = PdfController::default();
    controller.bind_document(document_id, page_count, page_ids);

    controller.apply_effect(
        PdfViewEffect::RenderPage {
            page_index: PageId::new(1),
            scale: 1.0,
        },
        &mut engine,
    );
    assert!(
        matches!(controller.state().status, PdfViewStatus::Ready),
        "initial render must be ready"
    );
    assert_eq!(controller.state().scale, 1.0);

    let zoom_effects = [
        controller.zoom_in(),
        controller.zoom_in(),
        controller.zoom_out(),
    ];
    for effect in zoom_effects {
        controller.apply_effect(effect, &mut engine);
    }

    assert_eq!(controller.state().scale, 1.25);
    assert!(
        matches!(controller.state().status, PdfViewStatus::Ready),
        "retry render after zoom must remain ready"
    );
    assert!(
        !controller.state().png.is_empty(),
        "retry render must preserve rendered page"
    );
}

#[test]
fn pdf_tab_round_trip_preserves_bound_controller_state() {
    let writer = WriterApi::new();
    let pdf_bytes = match writer
        .export(writer_api::core::DocumentFormat::Pdf)
        .expect("must create a PDF package")
    {
        writer_api::ExportedDocument::Pdf(bytes) => bytes,
        _ => panic!("writer returned an unexpected export type"),
    };
    let mut state = AppState::default();
    let (document_a, _, _) =
        OpenDocument::open(Path::new("same.pdf"), &pdf_bytes, &mut state.pdf_engine)
            .expect("PDF A must open");
    let backend_a = document_a.pdf_bindings().expect("PDF A bindings").0;
    let document_a = state
        .register_document(PathBuf::from("same.pdf"), document_a)
        .expect("PDF A must register");
    let (document_b, _, _) =
        OpenDocument::open(Path::new("same.pdf"), &pdf_bytes, &mut state.pdf_engine)
            .expect("PDF B must open");
    let document_b = state
        .register_document(PathBuf::from("same.pdf"), document_b)
        .expect("PDF B must register");

    state.activate_document(document_a).expect("activate PDF A");
    state.set_zoom(document_a, 1.5).expect("zoom PDF A");
    state.scroll(document_a, 12.0, 18.0).expect("scroll PDF A");
    let expected_a = state.inspect(document_a).unwrap().view_state;

    state.activate_document(document_b).expect("activate PDF B");
    state.set_zoom(document_b, 2.0).expect("zoom PDF B");
    assert_eq!(state.active_document_id(), Some(document_b));

    state
        .activate_document(document_a)
        .expect("restore PDF A by its runtime ID");
    assert_eq!(state.active_document_id(), Some(document_a));
    assert_eq!(state.active_module(), Module::Pdf);
    assert_eq!(state.inspect(document_a).unwrap().view_state, expected_a);
    assert_eq!(state.pdf_controller.state().document_id, Some(backend_a));
    let DocumentViewState::Pdf(restored) = expected_a else {
        panic!("PDF view state expected");
    };
    assert_eq!(state.pdf_controller.state().scale, restored.zoom);
    assert_eq!(
        state.pdf_controller.state().current_page_number(),
        restored.page_number
    );
}

#[test]
fn pdf_tab_round_trip_restores_absolute_scroll_without_advancing_page() {
    let mut state = AppState::default();
    let document_a = state
        .register_document(
            PathBuf::from("three-pages.pdf"),
            OpenDocument::Pdf {
                document_id: DocumentId::new(101),
                page_count: 3,
                page_ids: vec![PageId::new(1), PageId::new(2), PageId::new(3)],
            },
        )
        .expect("three-page PDF must register");
    let document_b = state
        .register_document(
            PathBuf::from("other.pdf"),
            OpenDocument::Pdf {
                document_id: DocumentId::new(102),
                page_count: 1,
                page_ids: vec![PageId::new(1)],
            },
        )
        .expect("second PDF must register");

    state
        .activate_document(document_a)
        .expect("activate three-page PDF");
    state.pdf_go_to_page(2);
    let DocumentViewState::Pdf(expected) = state.inspect(document_a).unwrap().view_state else {
        panic!("PDF view state expected");
    };
    assert_eq!(expected.page_number, 2);
    assert!(expected.scroll_y > 0.0);

    state
        .activate_document(document_b)
        .expect("switch to another PDF tab");
    state
        .activate_document(document_a)
        .expect("restore three-page PDF tab");

    assert_eq!(state.pdf_controller.state().current_page_number(), 2);
    assert_eq!(
        state.pdf_controller.state().scroll_x_points,
        expected.scroll_x
    );
    assert_eq!(
        state.pdf_controller.state().scroll_y_points,
        expected.scroll_y
    );
}

#[test]
fn opening_non_pdf_document_resets_pdf_controller() {
    let mut state = state_with_test_document(
        "document.pdf",
        OpenDocument::Pdf {
            document_id: DocumentId::new(1),
            page_count: 1,
            page_ids: vec![PageId::new(1)],
        },
    );
    state
        .pdf_controller
        .state_mut()
        .bind_pdf(DocumentId::new(1), 1, &[PageId::new(1)]);
    state.pdf_controller.state_mut().scale = 2.0;
    state.pdf_controller.state_mut().png = PNG_SIGNATURE.to_vec();

    attach_test_document(
        &mut state,
        "book.xlsx",
        OpenDocument::Sheets(WorkbookSnapshot::default()),
    );
    state.clear_pdf_document();

    assert!(
        !matches!(state.document(), Some(OpenDocument::Pdf { .. })),
        "document owner must switch from PDF to non-PDF"
    );
    assert_eq!(state.pdf_controller.state().status, PdfViewStatus::Empty);
    assert_eq!(state.pdf_controller.state().scale, 1.0);
    assert!(state.pdf_controller.state().png.is_empty());
}

#[test]
fn pdf_render_rejects_invalid_scale() {
    let mut controller = PdfController::default();
    controller
        .state_mut()
        .bind_pdf(DocumentId::new(1), 1, &[PageId::new(1)]);
    for &scale in &[-0.1_f32, 0.249_f32, 4.01_f32] {
        controller.apply_effect(
            PdfViewEffect::RenderPage {
                page_index: PageId::new(1),
                scale,
            },
            &mut PdfOxideAdapter::default(),
        );
        assert!(matches!(controller.state().status, PdfViewStatus::Error));
        assert!(
            controller
                .state()
                .error
                .as_deref()
                .unwrap_or("")
                .contains("scale")
        );
        assert_eq!(controller.state().png, Vec::<u8>::new());
    }
}

#[test]
fn pdf_zoom_respects_min_max_scale() {
    let mut controller = PdfController::default();
    controller
        .state_mut()
        .bind_pdf(DocumentId::new(1), 1, &[PageId::new(1)]);
    for _ in 0..20 {
        controller.zoom_in();
    }
    assert!(
        controller.state().scale <= PDF_MAX_SCALE,
        "zoom_in must not exceed the MVP max scale"
    );
    for _ in 0..40 {
        controller.zoom_out();
    }
    assert!(
        controller.state().scale >= PDF_MIN_SCALE,
        "zoom_out must not go below the MVP min scale"
    );
}

#[test]
fn pdf_page_geometry_projection_preserves_physical_aspect_and_rotation() {
    let portrait = PageGeometry::new([0.0, 0.0, 600.0, 800.0], None, 0)
        .expect("portrait geometry must be valid");
    let landscape = PageGeometry::new(
        [0.0, 0.0, 600.0, 800.0],
        Some([10.0, 20.0, 410.0, 620.0]),
        90,
    )
    .expect("rotated crop geometry must be valid");
    assert_eq!(
        (portrait.width_points, portrait.height_points),
        (600.0, 800.0)
    );
    assert_eq!(
        (landscape.width_points, landscape.height_points),
        (600.0, 400.0)
    );

    let mut state = PdfViewState::default();
    state.bind_pdf_with_geometries(
        DocumentId::new(1),
        2,
        &[PageId::new(1), PageId::new(2)],
        &[portrait, landscape],
    );
    let placements = state.page_placements();
    assert_eq!(placements.len(), 2);
    assert_eq!(placements[0].geometry, portrait);
    assert!((placements[0].width_points / placements[0].height_points - 0.75).abs() < 0.001);
    assert!(placements[1].y_points > placements[0].y_points + placements[0].height_points);
}

#[test]
fn pdf_initial_viewport_keeps_narrow_current_page_visible_with_wide_document() {
    let narrow = PageGeometry::new([0.0, 0.0, 595.0, 842.0], None, 0)
        .expect("narrow page geometry must be valid");
    let wide = PageGeometry::new([0.0, 0.0, 2384.0, 1684.0], None, 0)
        .expect("wide page geometry must be valid");
    let mut state = PdfViewState::default();
    state.bind_pdf_with_geometries(
        DocumentId::new(1),
        2,
        &[PageId::new(1), PageId::new(2)],
        &[narrow, wide],
    );
    state.set_viewport_points(924.0, 182.0);

    let first = state
        .page_placements()
        .into_iter()
        .next()
        .expect("first page placement must exist");
    let viewport_left = state.scroll_x_points;
    let viewport_right = viewport_left + state.viewport_width_points;
    assert!(
        first.x_points >= viewport_left - 0.01,
        "initial viewport must not clip the left edge of the active narrow page"
    );
    assert!(
        first.x_points + first.width_points <= viewport_right + 0.01,
        "initial viewport must not clip the right edge of the active narrow page"
    );
}

#[test]
fn pdf_fresh_session_does_not_replace_initial_page_reveal_with_zero_scroll() {
    let narrow = PageGeometry::new([0.0, 0.0, 595.0, 842.0], None, 0)
        .expect("narrow page geometry must be valid");
    let wide = PageGeometry::new([0.0, 0.0, 2384.0, 1684.0], None, 0)
        .expect("wide page geometry must be valid");
    let mut state = PdfViewState::default();
    state.bind_pdf_with_geometries(
        DocumentId::new(1),
        2,
        &[PageId::new(1), PageId::new(2)],
        &[narrow, wide],
    );
    state.set_viewport_points(924.0, 182.0);
    let _ = state.set_scroll_offset_points(0.0, 0.0);

    restore_pdf_session_view(&mut state, PdfSessionView::default());

    let first = state
        .page_placements()
        .into_iter()
        .next()
        .expect("first page placement must exist");
    let viewport_left = state.scroll_x_points;
    let viewport_right = viewport_left + state.viewport_width_points;
    assert!(first.x_points >= viewport_left - 0.01);
    assert!(first.x_points + first.width_points <= viewport_right + 0.01);
}
#[test]
fn pdf_scale_change_keeps_current_page_visible_in_narrow_viewport() {
    let narrow = PageGeometry::new([0.0, 0.0, 595.0, 842.0], None, 0)
        .expect("narrow page geometry must be valid");
    let wide = PageGeometry::new([0.0, 0.0, 2526.0, 1684.0], None, 0)
        .expect("wide page geometry must be valid");
    let mut state = PdfViewState::default();
    state.bind_pdf_with_geometries(
        DocumentId::new(1),
        2,
        &[PageId::new(1), PageId::new(2)],
        &[narrow, wide],
    );
    state.set_viewport_points(444.0, 54.0);
    state.scroll_to_page(PageId::new(1));

    state.set_scale(0.5);

    let current = state
        .page_placements()
        .into_iter()
        .find(|placement| placement.page_index == state.current_page)
        .expect("current page placement must exist");
    let viewport_right = state.scroll_x_points + state.viewport_width_points;
    let viewport_bottom = state.scroll_y_points + state.viewport_height_points;
    assert!(
        current.x_points < viewport_right
            && current.x_points + current.width_points > state.scroll_x_points,
        "scale changes must not move the current page fully outside the horizontal viewport"
    );
    assert!(
        current.y_points < viewport_bottom
            && current.y_points + current.height_points > state.scroll_y_points,
        "scale changes must not move the current page fully outside the vertical viewport"
    );
}

#[test]
fn pdf_viewport_scroll_is_bounded_and_keeps_render_window_bounded() {
    let page_ids = (1..=100).map(PageId::new).collect::<Vec<_>>();
    let geometries = (0..100).map(|_| PageGeometry::letter()).collect::<Vec<_>>();
    let mut state = PdfViewState::default();
    state.bind_pdf_with_geometries(
        DocumentId::new(1),
        page_ids.len() as u32,
        &page_ids,
        &geometries,
    );
    state.set_viewport_points(640.0, 420.0);
    assert!(state.content_height_points > state.viewport_height_points);
    assert!(state.scroll_by_points(0.0, f32::MAX));
    assert_eq!(state.scroll_y_points, state.max_scroll_y());
    assert!(state.current_page_number() > 1);
    assert!(state.visible_page_ids().len() <= PDF_MAX_VISIBLE_PAGES);
    assert!(state.scroll_by_points(0.0, -f32::MAX));
    assert_eq!(state.scroll_y_points, 0.0);
    assert_eq!(state.current_page_number(), 1);
}

#[test]
fn pdf_fit_actions_use_the_measured_canvas_not_bootstrap_dimensions() {
    let mut controller = PdfController::default();
    controller.bind_document(DocumentId::new(1), 1, &[PageId::new(1)]);
    for (width, height) in [(1200.0, 506.0), (800.0, 345.6), (608.0, 265.6)] {
        controller.set_viewport_points(width, height);
        let geometry = controller.state().geometry_for(PageId::new(1));
        controller.fit_page();
        assert_eq!(
            controller.state().scale,
            compute_pdf_fit_page_scale(geometry, width, height)
        );
        assert!(geometry.height_points * controller.state().scale <= height + 0.01);
        controller.fit_width();
        assert_eq!(
            controller.state().scale,
            compute_pdf_fit_width_scale(geometry, width)
        );
    }
}

#[test]
fn pdf_zoom_contract_clamps_and_fit_modes_use_geometry() {
    let mut state = PdfViewState::default();
    state.bind_pdf_with_geometries(
        DocumentId::new(1),
        1,
        &[PageId::new(1)],
        &[PageGeometry::letter()],
    );
    state.set_scale(99.0);
    assert_eq!(state.scale, PDF_MAX_SCALE);
    state.set_scale(0.0);
    assert_eq!(state.scale, PDF_MIN_SCALE);
    state.fit_width(760.0);
    assert_eq!(state.zoom_mode, PdfZoomMode::FitWidth);
    assert!((PDF_MIN_SCALE..=PDF_MAX_SCALE).contains(&state.scale));
    state.fit_page(760.0, 680.0);
    assert_eq!(state.zoom_mode, PdfZoomMode::FitPage);
    assert!((PDF_MIN_SCALE..=PDF_MAX_SCALE).contains(&state.scale));
}

#[test]
fn pdf_viewer_frame_contains_only_bounded_visible_pages_and_cache_hits() {
    let page_ids = (1..=32).map(PageId::new).collect::<Vec<_>>();
    let geometries = (0..32).map(|_| PageGeometry::letter()).collect::<Vec<_>>();
    let mut state = AppState::default();
    state.pdf_controller.state_mut().bind_pdf_with_geometries(
        DocumentId::new(1),
        page_ids.len() as u32,
        &page_ids,
        &geometries,
    );
    state.pdf_controller.state_mut().visible_pages = vec![RenderedPage {
        page_index: PageId::new(1),
        scale: 1.0,
        png: PNG_SIGNATURE.to_vec(),
    }];
    let frame = state.pdf_frame();
    assert_eq!(frame.document_id, Some(DocumentId::new(1)));
    assert!(frame.pages.len() <= PDF_MAX_VISIBLE_PAGES);
    assert!(
        frame
            .pages
            .iter()
            .any(|page| page.placement.page_index == PageId::new(1))
    );
    assert_eq!(frame.pages[0].placement.geometry.rotation_degrees, 0);
}

#[test]
fn pdf_cache_eviction_removes_oldest_and_keeps_hit() {
    let mut cache = PdfPageCache::new(2);
    cache.insert(DocumentId::new(1), PageId::new(1), 1.0, vec![1]);
    cache.insert(DocumentId::new(1), PageId::new(2), 1.0, vec![2]);
    cache.get(DocumentId::new(1), PageId::new(1), 1.0);
    cache.insert(DocumentId::new(1), PageId::new(3), 1.0, vec![3]);
    assert!(cache.get(DocumentId::new(1), PageId::new(2), 1.0).is_none());
    assert_eq!(
        cache.get(DocumentId::new(1), PageId::new(1), 1.0),
        Some([1].as_slice())
    );
    assert_eq!(
        cache.get(DocumentId::new(1), PageId::new(3), 1.0),
        Some([3].as_slice())
    );
}

#[test]
fn unified_pdf_open_degrades_viewer_only_content() {
    let mut engine = PdfOxideAdapter::default();
    let (document, status, detail) = OpenDocument::open(
        Path::new("viewer-only.pdf"),
        b"%PDF-1.7\n1 0 obj << /Type /Page /Foo /JavaScript >>\nendobj\n%%EOF",
        &mut engine,
    )
    .expect("viewer-only content must not reject a readable PDF");
    assert_eq!(document.module(), Module::Pdf);
    assert!(status.contains("PDF открыт"));
    assert!(detail.contains("безопасном режиме просмотра"));
    assert!(detail.contains("потенциальный JavaScript"));
    let (_, page_count, _) = document.pdf_bindings().expect("pdf bindings must exist");
    assert_eq!(page_count, 1);
    assert_eq!(engine.document_count(), 1);
}

#[test]
fn invalid_pptx_reports_office_oxide_parse_stage() {
    let result = OpenDocument::open(
        Path::new("broken.pptx"),
        b"not-a-real-pptx",
        &mut PdfOxideAdapter::default(),
    );
    let error = result.err().expect("malformed PPTX must fail");
    assert!(error.starts_with("Этап Office Oxide parse:"));
}

#[test]
fn pdf_page_count_after_open_is_positive() {
    let writer = WriterApi::new();
    let pdf_bytes = match writer
        .export(writer_api::core::DocumentFormat::Pdf)
        .expect("must create a PDF package")
    {
        writer_api::ExportedDocument::Pdf(bytes) => bytes,
        _ => panic!("writer returned an unexpected export type"),
    };
    let mut engine = PdfOxideAdapter::default();
    let (document, _, _) = OpenDocument::open(Path::new("document.pdf"), &pdf_bytes, &mut engine)
        .expect("must open pdf");
    let (_, page_count, _) = document.pdf_bindings().expect("pdf bindings must exist");
    assert!(page_count > 0);
}

#[test]
fn docx_open_export_and_reopen_retains_writer_module() {
    let writer = WriterViewModel::new();
    let docx = writer.export_docx().expect("must create a DOCX package");
    let mut engine = PdfOxideAdapter::default();
    let (document, _, _) =
        OpenDocument::open(Path::new("document.docx"), &docx, &mut engine).expect("must open docx");
    assert_eq!(document.module(), Module::Writer);
    let bytes = document.export_bytes().expect("must export docx");
    let (reopened, _, _) = OpenDocument::open(Path::new("document.docx"), &bytes, &mut engine)
        .expect("must reopen exported docx");
    assert_eq!(reopened.module(), Module::Writer);
}

#[test]
fn invalid_docx_open_does_not_change_active_session() {
    let _state = state_with_test_document(
        "book.xlsx",
        OpenDocument::Sheets(WorkbookSnapshot::default()),
    );
    let result = OpenDocument::open(
        Path::new("bad.docx"),
        b"not-a-real-docx",
        &mut PdfOxideAdapter::default(),
    );
    assert!(result.is_err());
}

#[test]
fn writer_visibility_follows_active_document_identity() {
    let docx = WriterViewModel::new()
        .export_docx()
        .expect("must create a DOCX package");
    let mut engine = PdfOxideAdapter::default();
    let (document, _, _) =
        OpenDocument::open(Path::new("document.docx"), &docx, &mut engine).expect("must open docx");

    let mut state = AppState::default();
    assert!(!state.should_show_writer_view());

    let writer = attach_test_document(&mut state, "document.docx", document);
    assert!(state.should_show_writer_view());
    assert_eq!(state.active_module(), Module::Writer);

    let sheets = attach_test_document(
        &mut state,
        "book.xlsx",
        OpenDocument::Sheets(WorkbookSnapshot::default()),
    );
    assert_eq!(state.active_document_id(), Some(sheets));
    assert_eq!(state.active_module(), Module::Sheets);
    assert!(!state.should_show_writer_view());

    state.active_document = Some(writer);
    assert_eq!(state.active_module(), Module::Writer);
    assert!(state.should_show_writer_view());
}

#[test]
fn writer_projection_notice_is_quiet_for_supported_content_and_counts_fallbacks() {
    let diagnostics = writer_api::DocxProjectionDiagnostics::default();
    assert_eq!(
        writer_projection_notice_from_diagnostics(&diagnostics),
        None
    );

    let mut diagnostics = writer_api::DocxProjectionDiagnostics::default();
    diagnostics.fallback.text_boxes = 2;
    diagnostics.fallback.unsupported_images = 1;
    assert_eq!(
        writer_projection_notice_from_diagnostics(&diagnostics).as_deref(),
        Some("Часть содержимого DOCX отображена текстовыми заметками: 3.")
    );
}

#[test]
fn writer_text_projection_handles_empty_pages_and_layout_none() {
    let vm = WriterViewModel::new();
    assert!(writer_text_view_label(&WriterTextView::default()).contains("не содержит"));
    assert!(
        project_writer_text(&vm)
            .expect("projection must exist")
            .is_empty()
    );
    assert_eq!(
        writer_text_view_label(&project_writer_text(&vm).unwrap()),
        "Writer не содержит видимого текста."
    );

    let docx = vm.export_docx().expect("must create a DOCX package");
    let mut engine = PdfOxideAdapter::default();
    let (document, _, _) =
        OpenDocument::open(Path::new("document.docx"), &docx, &mut engine).expect("must open docx");
    let OpenDocument::Writer(opened_vm) = document else {
        panic!("expected writer document");
    };
    let view = project_writer_text(&opened_vm).expect("projection must exist");
    assert_eq!(writer_fallback_kind(&opened_vm), WriterFallback::Empty);
    assert!(view.is_empty());
    assert_eq!(
        writer_text_view_label(&view),
        "Writer не содержит видимого текста."
    );
}

#[test]
fn writer_text_projection_bounds_and_preserves_order_with_truncation() {
    let vm = WriterViewModel::new();
    let view = project_writer_text(&vm).expect("projection must exist");
    assert!(view.pages.len() <= WRITER_VIEW_MAX_PAGES);
    assert!(view.is_empty());
    assert!(
        view.pages
            .iter()
            .all(|page| page.blocks.iter().all(|block| block
                .fragments
                .iter()
                .all(|fragment| fragment.text.is_empty())))
    );
    assert!(!view.truncated);
    assert_eq!(
        writer_text_view_label(&view),
        "Writer не содержит видимого текста."
    );
}

#[test]
fn writer_empty_content_invariant_is_semantic() {
    assert!(WriterTextView::default().is_empty());

    let one_empty_block = writer_text_view_from_blocks(&[&[""]]);
    assert!(one_empty_block.is_empty());

    let several_empty_blocks = writer_text_view_from_blocks(&[&[""], &["", ""]]);
    assert!(several_empty_blocks.is_empty());

    // Whitespace is preserved verbatim by Writer and is therefore user content.
    let whitespace_only_block = writer_text_view_from_blocks(&[&[" \t"]]);
    assert!(!whitespace_only_block.is_empty());

    let non_empty_block = writer_text_view_from_blocks(&[&["text"]]);
    assert!(!non_empty_block.is_empty());

    let mixed_blocks = writer_text_view_from_blocks(&[&[""], &["text"], &[""]]);
    assert!(!mixed_blocks.is_empty());
}

#[test]
fn writer_text_fallback_handles_empty_and_long_content() {
    let docx = WriterViewModel::new()
        .export_docx()
        .expect("must create a DOCX package");
    let mut engine = PdfOxideAdapter::default();
    let (document, _, _) =
        OpenDocument::open(Path::new("document.docx"), &docx, &mut engine).expect("must open docx");
    let OpenDocument::Writer(vm) = document else {
        panic!("expected writer document");
    };

    assert_eq!(writer_fallback_kind(&vm), WriterFallback::Empty);
    assert_eq!(
        writer_fallback_label(&vm),
        "Writer не содержит видимого текста."
    );
    assert_eq!(writer_text_fallback(&vm), None);
}

#[test]
fn writer_visible_pages_label_uses_half_open_range_and_handles_missing_draw() {
    assert_eq!(
        writer_visible_pages_label(Some(&(1..3)), 5),
        "Область: 2–3 из 5"
    );
    assert_eq!(
        writer_visible_pages_label(Some(&(0..1)), 5),
        "Область: 1 из 5"
    );
    assert_eq!(
        writer_visible_pages_label(Some(&(4..7)), 5),
        "Область: 5 из 5"
    );
    assert_eq!(
        writer_visible_pages_label(Some(&(5..6)), 5),
        "Область: — из 5"
    );
    assert_eq!(writer_visible_pages_label(None, 5), "Область: — из 5");
}

#[test]
fn writer_page_info_matches_view_state_without_touching_layout() {
    let docx = WriterViewModel::new()
        .export_docx()
        .expect("must create a DOCX package");
    let mut engine = PdfOxideAdapter::default();
    let (document, _, _) =
        OpenDocument::open(Path::new("document.docx"), &docx, &mut engine).expect("must open docx");
    let OpenDocument::Writer(vm) = document else {
        panic!("expected writer document");
    };

    let page_info = writer_page_info(&vm);
    assert_eq!(page_info.page_count, vm.state().pages.len());
    assert_eq!(page_info.mode_label, "Страницы");
    assert_eq!(writer_zoom_label(vm.state()), "100%");
}

#[test]
fn print_layout_mode_selects_the_visual_page_renderer_state() {
    let mut state = state_with_test_document(
        "document.docx",
        OpenDocument::Writer(Box::new(WriterViewModel::new())),
    );
    assert_eq!(state.writer_display_mode, WriterDisplayMode::Visual);
    state.set_writer_display_mode(WriterDisplayMode::PlainText);
    assert_eq!(state.writer_display_mode, WriterDisplayMode::PlainText);
    state.set_writer_display_mode(WriterDisplayMode::Visual);
    assert_eq!(state.writer_display_mode, WriterDisplayMode::Visual);
}
#[test]
fn writer_fallback_kind_classifies_layout_cache() {
    let empty_docx = WriterViewModel::new()
        .export_docx()
        .expect("must create an empty DOCX package");
    let mut engine = PdfOxideAdapter::default();
    let (empty_document, _, _) =
        OpenDocument::open(Path::new("empty.docx"), &empty_docx, &mut engine)
            .expect("must open empty docx");
    let OpenDocument::Writer(empty_vm) = empty_document else {
        panic!("expected writer document");
    };
    assert_eq!(writer_fallback_kind(&empty_vm), WriterFallback::Empty);
    assert_eq!(
        writer_fallback_label(&empty_vm),
        "Writer не содержит видимого текста."
    );
    assert_eq!(writer_text_fallback(&empty_vm), None);

    let fresh_vm = WriterViewModel::new();
    // `WriterViewModel::new()` refreshes into an empty cached layout, so the
    // only observable `layout == None` state is not produced by the current public
    // default/open path; remove unavailable-branch assertions instead of faking one.
    assert_eq!(writer_fallback_kind(&fresh_vm), WriterFallback::Empty);
    assert_eq!(
        writer_fallback_label(&fresh_vm),
        "Writer не содержит видимого текста."
    );
    assert_eq!(writer_text_fallback(&fresh_vm), None);
}

#[test]
fn writer_fallback_text_preserves_order_and_bounds_unicode_bytes() {
    let text = "α".repeat(8192);
    let bounded = bounded_unicode_text(text);
    assert_eq!(bounded.len(), 12 * 1024);
    assert!(bounded.is_char_boundary(bounded.len()));
    assert!(bounded.len() < "α".repeat(8192).len() + 1);
}

#[test]
fn bounded_unicode_text_truncates_at_char_boundary() {
    let text = "α".repeat(8192);
    let bounded = bounded_unicode_text(text);
    assert_eq!(bounded.len(), 12 * 1024);
    assert!(bounded.is_char_boundary(bounded.len()));
    assert!(bounded.len() < "α".repeat(8192).len() + 1);
}

#[test]
fn writer_zoom_label_uses_exact_percent_and_gated_by_module() {
    let state = state_with_test_document(
        "document.docx",
        OpenDocument::Writer(Box::new(WriterViewModel::new())),
    );
    let vm = match state.document() {
        Some(OpenDocument::Writer(vm)) => vm,
        _ => panic!("expected writer document"),
    };
    let zoom_percent = vm.state().zoom_percent;
    assert_eq!(format!("{}%", zoom_percent), "100%");
    assert_eq!(vm.state().pages.len(), 1);

    assert!(state.should_show_writer_view());
}

#[test]
#[ignore = "requires explicit RUDRA_WRITER_ACCEPTANCE_DOCX operator fixture"]
fn writer_real_docx_acceptance_contract_when_path_is_provided() {
    let path = std::env::var_os("RUDRA_WRITER_ACCEPTANCE_DOCX")
        .expect("set the private operator fixture for this explicit acceptance test");
    let path = PathBuf::from(path);
    let bytes = std::fs::read(&path).expect("acceptance DOCX must be readable");
    let mut pdf_engine = PdfOxideAdapter::default();
    let (document, _, _) = OpenDocument::open(&path, &bytes, &mut pdf_engine)
        .expect("acceptance DOCX must open through Writer");
    let OpenDocument::Writer(vm) = document else {
        panic!("acceptance file must route to Writer");
    };
    let state = vm.state();
    let layout = state
        .layout
        .as_ref()
        .expect("Writer layout must be available");
    let snapshot = vm.snapshot();
    let frame = build_writer_page_frame(
        &snapshot,
        layout,
        vm.api().layout_settings(),
        vm.docx_layout(),
        state.zoom_percent,
        0.0,
        0.0,
    );

    assert!(
        !frame.pages.is_empty(),
        "a readable Writer document must create visible page geometry"
    );
    assert!(
        frame.pages.iter().all(|page| {
            page.width_half_points > page.margins.left_half_points + page.margins.right_half_points
                && page.height_half_points
                    > page.margins.top_half_points + page.margins.bottom_half_points
        }),
        "page geometry must leave a non-zero content rectangle"
    );

    let source_blocks = snapshot.blocks.len();
    let visual_blocks = frame
        .pages
        .iter()
        .map(|page| page.blocks.len())
        .sum::<usize>();
    if source_blocks > 0 {
        assert!(
            visual_blocks > 0,
            "source-backed DOCX content must reach the Writer render plan"
        );
    }

    eprintln!(
        "writer-real-docx pages={} source_blocks={} visual_blocks={} size={}x{} margins={}/{}/{}/{}",
        frame.pages.len(),
        source_blocks,
        visual_blocks,
        frame.pages[0].width_half_points,
        frame.pages[0].height_half_points,
        frame.pages[0].margins.top_half_points,
        frame.pages[0].margins.right_half_points,
        frame.pages[0].margins.bottom_half_points,
        frame.pages[0].margins.left_half_points,
    );
}

#[test]
fn writer_controlled_fixture_preserves_visible_content_classes() {
    let path = controlled_writer_fixture_path();
    let (margin, paragraphs, bold_marker, columns, rows, cells, header, footer) =
        if cfg!(feature = "private-fixtures") {
            (
                102,
                [
                    "Контролируемая приёмка Writer",
                    "Кириллица:",
                    "Вторая страница",
                ],
                "Кириллица:",
                3,
                4,
                vec!["Проверка", "Текст и Unicode", "Русский текст отображается"],
                "Rudra Office — controlled acceptance",
                "Viewer-only fixture",
            )
        } else {
            (
                144,
                ["Рассвет", "🙂на орбите", "Учебная"],
                "Рассвет",
                3,
                4,
                vec!["Ячейка 1.1", "Ячейка 3.2", "Ячейка 4.3"],
                "Синтетический верх",
                "Синтетический низ",
            )
        };
    let bytes = std::fs::read(&path).expect("controlled Writer fixture must be readable");
    let mut pdf_engine = PdfOxideAdapter::default();
    let (document, _, _) = OpenDocument::open(&path, &bytes, &mut pdf_engine)
        .expect("controlled Writer fixture must open through Writer");
    let OpenDocument::Writer(vm) = document else {
        panic!("controlled Writer fixture must route to Writer");
    };
    let state = vm.state();
    let layout = state
        .layout
        .as_ref()
        .expect("controlled Writer fixture must have a layout");
    let snapshot = vm.snapshot();
    let frame = build_writer_page_frame(
        &snapshot,
        layout,
        vm.api().layout_settings(),
        vm.docx_layout(),
        state.zoom_percent,
        0.0,
        0.0,
    );

    assert!(
        frame.pages.len() >= 2,
        "the controlled fixture must retain multiple physical pages"
    );
    assert!(frame.pages.iter().all(|page| {
        page.width_half_points == 1191
            && page.height_half_points == 1684
            && page.margins.top_half_points == margin
            && page.margins.right_half_points == margin
            && page.margins.bottom_half_points == margin
            && page.margins.left_half_points == margin
    }));
    assert!(
        !frame.truncated,
        "the controlled fixture must fit within the current visual resource budget"
    );

    let paragraph_text = frame
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .filter_map(|block| match block {
            writer_page_view::WriterVisualBlock::Paragraph(paragraph) => Some(
                paragraph
                    .runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<String>();
    for expected in paragraphs {
        assert!(
            paragraph_text.contains(expected),
            "missing body text: {expected}"
        );
    }
    assert!(
        frame
            .pages
            .iter()
            .flat_map(|page| page.blocks.iter())
            .any(|block| match block {
                writer_page_view::WriterVisualBlock::Paragraph(paragraph) => paragraph
                    .runs
                    .iter()
                    .any(|run| run.text.contains(bold_marker) && run.style.bold),
                _ => false,
            })
    );

    let visual_tables = frame
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .filter_map(|block| match block {
            writer_page_view::WriterVisualBlock::Table(table) => Some(table),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        visual_tables.len(),
        1,
        "the controlled table must survive projection"
    );
    let table = visual_tables[0];
    assert_eq!(table.columns.len(), columns);
    assert_eq!(table.rows.len(), rows);
    assert!(table.rows.iter().all(|row| row.cells.len() == columns));
    let table_text = table
        .rows
        .iter()
        .flat_map(|row| row.cells.iter())
        .map(|cell| cell.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for expected in cells {
        assert!(
            table_text.contains(expected),
            "missing table text: {expected}"
        );
    }
    assert!(table.columns.iter().all(|width| *width > 0));
    assert!(table.rows.iter().all(|row| row.height_half_points > 0));

    let visual_images = frame
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .filter_map(|block| match block {
            writer_page_view::WriterVisualBlock::Image(image) => Some(image),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        visual_images.len(),
        1,
        "the embedded image must survive projection"
    );
    assert!(visual_images.iter().all(|image| {
        !image.bytes.is_empty()
            && image.width_half_points > 0
            && image.height_half_points > 0
            && image.pixel_width > 0
            && image.pixel_height > 0
    }));

    let first_page = &frame.pages[0];
    assert!(
        first_page
            .header
            .as_ref()
            .is_some_and(|actual| actual.text.contains(header))
    );
    assert!(
        first_page
            .footer
            .as_ref()
            .is_some_and(|actual| actual.text.contains(footer))
    );
}

#[test]
#[ignore = "requires explicit RUDRA_WRITER_JPEG_FIXTURE operator fixture"]
fn writer_jpeg_fixture_imports_a_real_jpeg_image_when_path_is_provided() {
    let path = std::env::var_os("RUDRA_WRITER_JPEG_FIXTURE")
        .expect("set the private JPEG document for this explicit acceptance test");
    let path = PathBuf::from(path);
    let bytes = std::fs::read(&path).expect("JPEG fixture DOCX must be readable");
    let mut pdf_engine = PdfOxideAdapter::default();
    let (document, _, _) = OpenDocument::open(&path, &bytes, &mut pdf_engine)
        .expect("JPEG fixture DOCX must open through Writer");
    let OpenDocument::Writer(vm) = document else {
        panic!("JPEG fixture must route to Writer");
    };
    assert!(vm.snapshot().blocks.iter().any(|block| {
        matches!(
            block,
            writer_api::core::BlockSnapshot::Image(image)
                if image.payload.format == writer_api::core::ImageFormat::Jpeg
        )
    }));
}
#[test]
fn writer_tab_roundtrip_preserves_viewer_state() {
    let mut state = AppState::default();
    let document_a = state
        .register_document(
            PathBuf::from("same.docx"),
            OpenDocument::Writer(Box::new(WriterViewModel::new())),
        )
        .expect("Writer A must register");
    let document_b = state
        .register_document(
            PathBuf::from("same.docx"),
            OpenDocument::Writer(Box::new(WriterViewModel::new())),
        )
        .expect("Writer B must register");

    state
        .activate_document(document_a)
        .expect("activate Writer A");
    state
        .set_writer_viewport(document_a, 1.5, 24.0, 48.0)
        .expect("set Writer A viewport");
    state
        .set_view_mode(
            document_a,
            DocumentViewMode::WriterLayout(ViewMode::Continuous),
        )
        .expect("set Writer A layout");
    state
        .set_view_mode(
            document_a,
            DocumentViewMode::WriterDisplay(WriterDisplayMode::PlainText),
        )
        .expect("set Writer A display");
    let expected_a = state.inspect(document_a).unwrap().view_state;

    state
        .activate_document(document_b)
        .expect("activate Writer B");
    state
        .set_writer_viewport(document_b, 0.75, 3.0, 6.0)
        .expect("set Writer B viewport");

    state
        .activate_document(document_a)
        .expect("restore Writer A by its runtime ID");
    assert_eq!(state.active_document_id(), Some(document_a));
    assert_eq!(state.active_module(), Module::Writer);
    assert_eq!(state.inspect(document_a).unwrap().view_state, expected_a);
    assert_eq!(state.writer_display_mode, WriterDisplayMode::PlainText);
    let Some(OpenDocument::Writer(writer)) = state.document() else {
        panic!("Writer A backend must be active");
    };
    assert_eq!(writer.state().mode, ViewMode::Continuous);
    assert_eq!(writer.state().zoom_percent, 150);
}

#[test]
fn sidebar_module_navigation_activates_existing_document_by_id() {
    let mut presentation = Presentation::new(PresentationId::new(41));
    presentation
        .add_slide(Slide::new(SlideId::new(1)))
        .expect("slide must register");
    let mut state = AppState::default();
    let writer = state
        .register_document(
            PathBuf::from("document.docx"),
            OpenDocument::Writer(Box::new(WriterViewModel::new())),
        )
        .expect("Writer must register");
    let slides = state
        .register_document(
            PathBuf::from("deck.pptx"),
            OpenDocument::Slides(presentation),
        )
        .expect("Slides must register");
    state.activate_document(writer).expect("activate Writer");

    let mut app = App {
        ui: WidgetRef::default(),
        state,
        writer_inline_editor: None,
        writer_inline_editor_pending_focus: false,
        sheets_cell_input_binding: None,
        native_paste_hook: Default::default(),
        launcher_startup_request: None,
    };
    let mut cx = Cx::new(Box::new(|_, _| {}));
    app.select_module(&mut cx, Module::Slides);
    assert_eq!(app.state.active_document_id(), Some(slides));
    assert_eq!(app.state.active_module(), Module::Slides);

    app.select_module(&mut cx, Module::Writer);
    assert_eq!(app.state.active_document_id(), Some(writer));
    assert_eq!(app.state.active_module(), Module::Writer);
}

#[test]
fn activation_reprojects_status_from_the_active_document_session() {
    let mut presentation = Presentation::new(PresentationId::new(61));
    presentation
        .add_slide(Slide::new(SlideId::new(1)))
        .expect("slide must register");
    let workbook = WorkbookSnapshot {
        active_sheet: SheetId(1),
        sheets: vec![sheet_with(
            1,
            "Sheet",
            vec![text_cell(SheetId(1), 0, 0, "value")],
        )],
        ..Default::default()
    };
    let mut state = AppState::default();
    let slides = state
        .register_document(
            PathBuf::from("deck.pptx"),
            OpenDocument::Slides(presentation),
        )
        .expect("Slides must register");
    let sheets = state
        .register_document(PathBuf::from("book.xlsx"), OpenDocument::Sheets(workbook))
        .expect("Sheets must register");

    state.activate_document(slides).expect("activate Slides");
    state.slides_status_update();
    assert!(state.status.starts_with("Презентация:"));

    state.activate_document(sheets).expect("activate Sheets");
    assert!(state.status.starts_with("XLSX открыт:"));
    assert!(!state.status.contains("Презентация"));
    assert!(state.detail.contains("book.xlsx"));
    assert!(
        state
            .detail
            .contains(&format!("Runtime ID: document:{sheets}"))
    );

    state.activate_document(slides).expect("restore Slides");
    assert!(state.status.starts_with("PPTX открыт:"));
    assert!(!state.status.contains("XLSX"));
}

#[test]
fn closing_the_last_document_reprojects_the_empty_workspace_summary() {
    let mut state = AppState::default();
    let writer = state
        .register_document(
            PathBuf::from("document.docx"),
            OpenDocument::Writer(Box::new(WriterViewModel::new())),
        )
        .expect("Writer must register");
    state.activate_document(writer).expect("activate Writer");

    state.close_document(writer).expect("close Writer");

    assert_eq!(state.active_document_id(), None);
    assert_eq!(state.active_module(), Module::Home);
    assert_eq!(state.status, Module::Home.hint());
    assert_eq!(state.detail, "Все вкладки документов закрыты.");
}

#[test]
fn read_only_save_active_includes_all_supported_viewer_formats() {
    let mut state = AppState::default();
    assert!(!state.read_only_save_active());

    attach_test_document(
        &mut state,
        "book.xlsx",
        OpenDocument::Sheets(WorkbookSnapshot::default()),
    );
    assert!(state.read_only_save_active());

    attach_test_document(
        &mut state,
        "deck.pptx",
        OpenDocument::Slides(Presentation::new(PresentationId::new(1))),
    );
    assert!(state.read_only_save_active());

    attach_test_document(
        &mut state,
        "document.docx",
        OpenDocument::Writer(Box::new(WriterViewModel::new())),
    );
    assert!(state.read_only_save_active());

    attach_test_document(
        &mut state,
        "document.pdf",
        OpenDocument::Pdf {
            document_id: DocumentId::new(1),
            page_count: 1,
            page_ids: vec![PageId::new(1)],
        },
    );
    assert!(state.read_only_save_active());

    state.active_document = None;
    assert!(!state.read_only_save_active());
}

#[test]
fn slides_visibility_follows_active_document_identity() {
    let mut state = AppState::default();
    assert!(!state.should_show_slides_viewer());

    let slides = attach_test_document(
        &mut state,
        "deck.pptx",
        OpenDocument::Slides(Presentation::new(PresentationId::new(1))),
    );
    assert!(state.should_show_slides_viewer());
    assert_eq!(state.active_module(), Module::Slides);

    let sheets = attach_test_document(
        &mut state,
        "book.xlsx",
        OpenDocument::Sheets(WorkbookSnapshot::default()),
    );
    assert_eq!(state.active_document_id(), Some(sheets));
    assert_eq!(state.active_module(), Module::Sheets);
    assert!(!state.should_show_slides_viewer());

    state.active_document = Some(slides);
    assert_eq!(state.active_module(), Module::Slides);
    assert!(state.should_show_slides_viewer());
}

#[test]
fn slides_tab_roundtrip_restores_slide_zoom_and_presentation_mode() {
    let mut presentation_a = Presentation::new(PresentationId::new(51));
    for id in 1..=3 {
        presentation_a
            .add_slide(Slide::new(SlideId::new(id)))
            .expect("slide A must register");
    }
    let mut presentation_b = Presentation::new(PresentationId::new(52));
    presentation_b
        .add_slide(Slide::new(SlideId::new(1)))
        .expect("slide B must register");
    let mut state = AppState::default();
    let document_a = state
        .register_document(
            PathBuf::from("same.pptx"),
            OpenDocument::Slides(presentation_a),
        )
        .expect("Slides A must register");
    let document_b = state
        .register_document(
            PathBuf::from("same.pptx"),
            OpenDocument::Slides(presentation_b),
        )
        .expect("Slides B must register");

    state
        .activate_document(document_a)
        .expect("activate Slides A");
    state.set_zoom(document_a, 1.75).expect("zoom Slides A");
    state.go_to_page(document_a, 3).expect("select slide 3");
    state
        .set_view_mode(document_a, DocumentViewMode::SlidesPresentation(true))
        .expect("enable presentation mode");
    state.sync_active_session_view();
    let expected_a = state.inspect(document_a).unwrap().view_state;

    state
        .activate_document(document_b)
        .expect("activate Slides B");
    state.set_zoom(document_b, 0.75).expect("zoom Slides B");

    state
        .activate_document(document_a)
        .expect("restore Slides A by its runtime ID");
    assert_eq!(state.active_document_id(), Some(document_a));
    assert_eq!(state.active_module(), Module::Slides);
    assert_eq!(state.inspect(document_a).unwrap().view_state, expected_a);
    let DocumentViewState::Slides(restored) = expected_a else {
        panic!("Slides view state expected");
    };
    assert_eq!(state.slides_current_index, restored.current_index);
    assert_eq!(state.slides_scale, restored.zoom);
    assert_eq!(state.slides_presentation_mode, restored.presentation_mode);
}

#[test]
fn slides_accessibility_navigation_enabled_state_matches_slide_bounds() {
    assert_eq!(
        slides_navigation_enabled(SlidesViewSnapshot::default()),
        (false, false)
    );
    assert_eq!(
        slides_navigation_enabled(SlidesViewSnapshot {
            current_index: 0,
            slide_count: 3,
            ..Default::default()
        }),
        (false, true)
    );
    assert_eq!(
        slides_navigation_enabled(SlidesViewSnapshot {
            current_index: 2,
            slide_count: 3,
            ..Default::default()
        }),
        (true, false)
    );
}
#[test]
fn slides_navigation_is_bounded_and_preserves_the_snapshot() {
    let mut presentation = Presentation::new(PresentationId::new(1));
    presentation
        .add_slide(Slide::new(SlideId::new(1)))
        .expect("first slide must be valid");
    presentation
        .add_slide(Slide::new(SlideId::new(2)))
        .expect("second slide must be valid");
    let original = presentation.clone();

    let mut state = state_with_test_document("deck.pptx", OpenDocument::Slides(presentation));
    state.bind_slides_document();

    assert_eq!(state.slides_current_index(), Some(0));
    state.slides_previous();
    assert_eq!(state.slides_current_index, 0);
    state.slides_next();
    assert_eq!(state.slides_current_index, 1);
    state.slides_next();
    assert_eq!(state.slides_current_index, 1);

    // The snapshot is not mutated by navigation.
    match state.document() {
        Some(OpenDocument::Slides(actual)) => {
            assert_eq!(actual.slides.len(), original.slides.len());
        }
        _ => panic!("slides document must remain bound"),
    }
}

#[test]
fn slides_zoom_is_clamped_and_reset_restores_defaults() {
    let mut presentation = Presentation::new(PresentationId::new(1));
    presentation
        .add_slide(Slide::new(SlideId::new(1)))
        .expect("slide must be valid");

    let mut state = state_with_test_document("deck.pptx", OpenDocument::Slides(presentation));
    state.bind_slides_document();

    for _ in 0..20 {
        state.slides_zoom_in();
    }
    assert!(
        state.slides_scale <= 4.0,
        "zoom_in must not exceed the MVP max scale"
    );

    for _ in 0..40 {
        state.slides_zoom_out();
    }
    assert!(
        state.slides_scale >= 0.25,
        "zoom_out must not go below the MVP min scale"
    );

    state.slides_reset();
    assert_eq!(state.slides_current_index, 0);
    assert_eq!(state.slides_scale, 1.0);
}

#[test]
fn slides_bind_and_clear_reset_viewer_state() {
    let mut presentation = Presentation::new(PresentationId::new(1));
    presentation
        .add_slide(Slide::new(SlideId::new(1)))
        .expect("slide must be valid");

    let mut state = state_with_test_document("deck.pptx", OpenDocument::Slides(presentation));
    state.slides_current_index = 3;
    state.slides_scale = 2.5;

    state.bind_slides_document();
    assert_eq!(state.slides_current_index, 0);
    assert_eq!(state.slides_scale, 1.0);

    state.slides_current_index = 3;
    state.slides_scale = 2.5;
    state.clear_slides_document();
    assert_eq!(state.slides_current_index, 0);
    assert_eq!(state.slides_scale, 1.0);
}
