use std::ffi::CString;

#[test]
fn test_create_context() {
    let initial_peer = CString::new("test").unwrap();
    let mut ptr: *mut crate::NetworkContext = std::ptr::null_mut();
    
    unsafe {
        assert_eq!(crate::create_context(initial_peer.as_ptr(), &mut ptr), 0);
    };
    
    assert!(!ptr.is_null());

    unsafe { crate::destroy_context(ptr) };
}
