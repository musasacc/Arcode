use gpui::TestAppContext;

use super::*;

#[gpui::test]
fn editing_and_utf16_mapping(cx: &mut TestAppContext) {
    let field = cx.new(|cx| TextField::new("", cx));
    field.update(cx, |f, cx| {
        f.replace(0..0, "aé😀b", cx);
        assert_eq!(f.text(), "aé😀b");
        assert_eq!(f.offset_to_utf16(f.text.len()), 5);
        assert_eq!(f.offset_from_utf16(2), "aé".len());
        assert_eq!(f.offset_from_utf16(4), "aé😀".len());
        let end = f.text.len();
        assert_eq!(f.prev_boundary(end - 1), "aé".len());
        f.selection = 1..1;
        f.replace(0..1, "", cx);
        assert_eq!(f.text(), "é😀b");
    });
}
