//! SBI console driver, for text output


use core::fmt::{self, Write};
use core::panicking::panic;

struct Stdout;
pub static mut fn_console_putchar:usize=0; // 全局变量，用于记录控制台的地址
pub fn console_putchar(c: usize){
    unsafe{
        if fn_console_putchar==0 {
            panic!("fn_console_putchar not set  ");
        }
        let p= fn_console_putchar as *const ();
        let f: fn(usize)= core::mem::transmute(p);
        f(c);
    }
}

impl Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            console_putchar(c as usize);
        }
        Ok(())
    }
}

pub fn print(args: fmt::Arguments) {
    Stdout.write_fmt(args).unwrap();
}

#[macro_export]
/// print string macro
macro_rules! print {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print(format_args!($fmt $(, $($arg)+)?));
    }
}

#[macro_export]
/// println string macro
macro_rules! println {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print(format_args!(concat!($fmt, "\n") $(, $($arg)+)?));
    }
}
pub fn set_console_putchar(f: *const ()){
    unsafe {
        fn_console_putchar=f as usize;
    }
    unsafe{
        // println!("set_console_putchar:{:#x}",fn_console_putchar);
    }
}
