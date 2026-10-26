use pdf_core::{DocumentId, PageGeometry, PageGeometryError, PageId};
use std::collections::HashSet;

#[test]
fn ids_are_copyable_hashable_and_keep_their_values() {
    let document = DocumentId::new(42);
    let copied_document = document;
    let page = PageId::new(7);
    let copied_page = page;

    assert_eq!(document, copied_document);
    assert_eq!(document.value(), 42);
    assert_eq!(page, copied_page);
    assert_eq!(page.value(), 7);
    assert_eq!(document.to_string(), "42");

    let mut pages = HashSet::new();
    pages.insert(page);
    assert!(pages.contains(&copied_page));
}

#[test]
fn geometry_uses_crop_box_and_swaps_dimensions_for_quarter_turns() {
    let media_box = [0.0, 0.0, 612.0, 792.0];
    let crop_box = Some([10.0, 20.0, 210.0, 320.0]);

    let portrait = PageGeometry::new(media_box, crop_box, 0).unwrap();
    assert_eq!(portrait.width_points, 200.0);
    assert_eq!(portrait.height_points, 300.0);
    assert_eq!(portrait.rotation_degrees, 0);

    let clockwise = PageGeometry::new(media_box, crop_box, 90).unwrap();
    assert_eq!(clockwise.width_points, 300.0);
    assert_eq!(clockwise.height_points, 200.0);

    let upside_down = PageGeometry::new(media_box, crop_box, 180).unwrap();
    assert_eq!(upside_down.width_points, 200.0);
    assert_eq!(upside_down.height_points, 300.0);

    let counterclockwise = PageGeometry::new(media_box, crop_box, 270).unwrap();
    assert_eq!(counterclockwise.width_points, 300.0);
    assert_eq!(counterclockwise.height_points, 200.0);

    let letter = PageGeometry::letter();
    assert_eq!(letter.width_points, 612.0);
    assert_eq!(letter.height_points, 792.0);
}

#[test]
fn geometry_rejects_invalid_boxes_and_rotations() {
    assert_eq!(
        PageGeometry::new([0.0, 0.0, f32::INFINITY, 10.0], None, 0).unwrap_err(),
        PageGeometryError::InvalidMediaBox
    );
    assert_eq!(
        PageGeometry::new([0.0, 0.0, 10.0, 10.0], Some([0.0, 0.0, f32::NAN, 3.0]), 0).unwrap_err(),
        PageGeometryError::InvalidCropBox
    );
    assert_eq!(
        PageGeometry::new([0.0, 0.0, 10.0, 10.0], Some([0.0, 0.0, 11.0, 3.0]), 0).unwrap_err(),
        PageGeometryError::CropBoxOutsideMediaBox
    );
    assert_eq!(
        PageGeometry::new([0.0, 0.0, 10.0, 10.0], None, 45).unwrap_err(),
        PageGeometryError::InvalidRotation(45)
    );
    assert_eq!(
        PageGeometry::new([-f32::MAX, 0.0, f32::MAX, 1.0], None, 0).unwrap_err(),
        PageGeometryError::InvalidMediaBox
    );
    assert!(PageGeometry::new([0.0, 1.0, 0.0, 10.0], None, 0).is_err());
}
