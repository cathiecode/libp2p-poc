fn setup() {
    static INITIALIZED: std::sync::Once = std::sync::Once::new();

    INITIALIZED.call_once(|| {
        crate::init();
    });
}

#[test]
fn test_usecase_server() {
    setup();
    let mut context: *mut crate::NetworkContext = std::ptr::null_mut();
    
    unsafe {
        assert_eq!(crate::create_context(std::ptr::null(), &mut context), 0);
    };
    
    assert!(!context.is_null());

    unsafe {
        assert_eq!(crate::listen_mirror(context), 0);
    };

    unsafe { crate::destroy_context(context) };
}

#[test]
fn test_usecase_client() {
    let mut context: *mut crate::NetworkContext = std::ptr::null_mut();
    
    unsafe {
        assert_eq!(crate::create_context(std::ptr::null(), &mut context), 0);
    };
    
    assert!(!context.is_null());

    unsafe { crate::destroy_context(context) };
}
