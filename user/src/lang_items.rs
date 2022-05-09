/// # Rust语言相关模块
/// `user/src/lang_items.rs`
/// ## 实现函数
/// - 实现了panic函数来对接panic!宏
// 

use core::panic::PanicInfo;

/// 打印处出错信息后陷入死循环
#[panic_handler]    //通知编译器用panic函数来对接 panic! 宏
fn panic(panic_info:&PanicInfo) -> ! {
    let err = panic_info.message().unwrap();
    if let Some(location) = panic_info.location() {
        println!(
            "Panicked at {}:{}, {}",
            location.file(),
            location.line(),
            err
        );
    } else {
        println!("Panicked: {}", err);
    }
    loop {}
}
