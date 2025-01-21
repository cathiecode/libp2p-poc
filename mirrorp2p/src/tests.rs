fn setup() {
    static INITIALIZED: std::sync::Once = std::sync::Once::new();

    INITIALIZED.call_once(|| {
        crate::init();
    });
}

fn identity_alice() -> Vec<u8> {
    include_bytes!("./tests/alice.key").to_vec()
}

fn identity_bob() -> Vec<u8> {
    include_bytes!("./tests/bob.key").to_vec()
}

#[test]
fn test_usecase_server() {
    setup();
    let mut context: *mut crate::NetworkContext = std::ptr::null_mut();
    
    unsafe {
        assert_eq!(crate::create_context(identity_alice().as_ptr(), identity_alice().len() as u16, std::ptr::null(), std::ptr::null(), &mut context), 0);
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
        assert_eq!(crate::create_context(identity_alice().as_ptr(), identity_alice().len() as u16, std::ptr::null(), std::ptr::null(), &mut context), 0);
    };
    
    assert!(!context.is_null());

    unsafe { crate::destroy_context(context) };
}
