use owo_colors::OwoColorize;
use supports_color::Stream;

/// Whether stderr/stdout supports color output.
fn color_enabled() -> bool {
    supports_color::on(Stream::Stdout).is_some()
}

// ---------------------------------------------------------------------------
// Semantic style helpers — each returns a styled `String`.
// When color is disabled, they return the input unchanged.
// ---------------------------------------------------------------------------

macro_rules! style_fn {
    ($name:ident, $($method:ident).+) => {
        pub fn $name(text: &str) -> String {
            if color_enabled() {
                text.$($method()).+.to_string()
            } else {
                text.to_string()
            }
        }
    };
}

style_fn!(success, green);
style_fn!(error, red.bold);
style_fn!(warning, yellow);
style_fn!(bold, bold);
style_fn!(dimmed, dimmed);
style_fn!(cyan, cyan);
style_fn!(green, green);
style_fn!(magenta, magenta);
style_fn!(yellow, yellow);
style_fn!(red, red);
style_fn!(diff_add, green);
style_fn!(diff_del, red);
