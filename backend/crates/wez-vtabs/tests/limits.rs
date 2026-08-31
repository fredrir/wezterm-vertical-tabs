#[test]
fn zen_frame_bounds_match_protocol_limits() {
    assert_eq!(
        vtabs_zen::frame::MAX_SIDE,
        vtabs_protocol::limits::FRAME_MAX_SIDE
    );
    assert_eq!(
        vtabs_zen::frame::MAX_AREA,
        vtabs_protocol::limits::FRAME_MAX_AREA
    );
}
