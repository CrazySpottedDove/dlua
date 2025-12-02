#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        {
            use colored::Colorize;
            println!("{}", format!("[INFO] {}", format!($($arg)*)).green());
        }

    }
}
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        {
            use colored::Colorize;
            println!("{}", format!("[WARN] {}", format!($($arg)*)).yellow());
        }
    }
}
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        {
            use colored::Colorize;
            use std::process;
            println!("{}", format!("[ERROR] {}", format!($($arg)*)).red());
            process::exit(1);
        }
    }
}
