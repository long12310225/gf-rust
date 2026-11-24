//! Port of `util/gpage` (`gpage.go`).
//!
//! GoFrame's page builder is deprecated upstream but still part of v2.10.5.
//! The port keeps the byte-exact HTML output: Go's `html.EscapeString` escapes
//! exactly `&`, `<`, `>`, `"` and `'` as `&amp;`, `&lt;`, `&gt;`, `&#34;` and
//! `&#39;`, and this module does the same locally so `gf-util` does not need
//! `gf-encoding`/`gf-text` (ADR-036).

/// The default page name.
pub const DEFAULT_PAGE_NAME: &str = "page";
/// The placeholder replaced by the page number in the URL template.
pub const DEFAULT_PAGE_PLACE_HOLDER: &str = "{.page}";

/// Escapes the five characters Go's `html.EscapeString` escapes.
fn html_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '\'' => escaped.push_str("&#39;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&#34;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// A pagination manager; all attributes are public like Go's `Page`.
#[derive(Clone, Debug)]
pub struct Page {
    /// Total size.
    pub total_size: i64,
    /// Total page, which is automatically calculated.
    pub total_page: i64,
    /// Current page number, at least 1.
    pub current_page: i64,
    /// Custom URL template for page URL producing.
    pub url_template: String,
    /// CSS style name for the `a` tag.
    pub link_style: String,
    /// CSS style name for the `span` tag.
    pub span_style: String,
    /// CSS style name for the `select` tag.
    pub select_style: String,
    /// Tag name for the next page.
    pub next_page_tag: String,
    /// Tag name for the previous page.
    pub prev_page_tag: String,
    /// Tag name for the first page.
    pub first_page_tag: String,
    /// Tag name for the last page.
    pub last_page_tag: String,
    /// Tag string for the previous bar.
    pub prev_bar_tag: String,
    /// Tag string for the next bar.
    pub next_bar_tag: String,
    /// Page bar number for displaying.
    pub page_bar_num: i64,
    /// Ajax function name; ajax is enabled when not empty.
    pub ajax_action_name: String,
}

impl Page {
    /// Creates and returns a pagination manager.
    pub fn new(total_size: i64, page_size: i64, current_page: i64, url_template: &str) -> Page {
        let total_page = if page_size <= 0 {
            0
        } else {
            (total_size + page_size - 1) / page_size
        };
        Page {
            link_style: "GPageLink".to_owned(),
            span_style: "GPageSpan".to_owned(),
            select_style: "GPageSelect".to_owned(),
            prev_page_tag: "<".to_owned(),
            next_page_tag: ">".to_owned(),
            first_page_tag: "|<".to_owned(),
            last_page_tag: ">|".to_owned(),
            prev_bar_tag: "<<".to_owned(),
            next_bar_tag: ">>".to_owned(),
            total_size,
            total_page,
            current_page: if current_page == 0 { 1 } else { current_page },
            url_template: url_template.to_owned(),
            page_bar_num: 10,
            ajax_action_name: String::new(),
        }
    }

    /// Returns the HTML content for the next page.
    pub fn next_page(&self) -> String {
        if self.current_page < self.total_page {
            return self.get_link(self.current_page + 1, &self.next_page_tag, "");
        }
        format!(
            r#"<span class="{}">{}</span>"#,
            self.span_style, self.next_page_tag
        )
    }

    /// Returns the HTML content for the previous page.
    pub fn prev_page(&self) -> String {
        if self.current_page > 1 {
            return self.get_link(self.current_page - 1, &self.prev_page_tag, "");
        }
        format!(
            r#"<span class="{}">{}</span>"#,
            self.span_style, self.prev_page_tag
        )
    }

    /// Returns the HTML content for the first page.
    pub fn first_page(&self) -> String {
        if self.current_page == 1 {
            return format!(
                r#"<span class="{}">{}</span>"#,
                self.span_style, self.first_page_tag
            );
        }
        self.get_link(1, &self.first_page_tag, "")
    }

    /// Returns the HTML content for the last page.
    pub fn last_page(&self) -> String {
        if self.current_page == self.total_page {
            return format!(
                r#"<span class="{}">{}</span>"#,
                self.span_style, self.last_page_tag
            );
        }
        self.get_link(self.total_page, &self.last_page_tag, "")
    }

    /// Returns the HTML page bar content with link and span tags.
    pub fn page_bar(&self) -> String {
        let mut plus = self.page_bar_num / 2;
        if self.page_bar_num - plus + self.current_page > self.total_page {
            plus = self.page_bar_num - self.total_page + self.current_page;
        }
        let mut begin = self.current_page - plus + 1;
        if begin < 1 {
            begin = 1;
        }
        let mut bar_content = String::new();
        let mut index = begin;
        while index < begin + self.page_bar_num {
            if index <= self.total_page {
                if index != self.current_page {
                    let bar_text = index.to_string();
                    bar_content.push_str(&self.get_link(index, &bar_text, &bar_text));
                } else {
                    bar_content.push_str(&format!(
                        r#"<span class="{}">{}</span>"#,
                        self.span_style, index
                    ));
                }
            } else {
                break;
            }
            index += 1;
        }
        bar_content
    }

    /// Returns the select HTML content for pagination.
    pub fn select_bar(&self) -> String {
        let mut bar_content = format!(
            r#"<select name="{}" onchange="window.location.href=this.value">"#,
            self.select_style
        );
        let mut index = 1;
        while index <= self.total_page {
            if index == self.current_page {
                bar_content.push_str(&format!(
                    r#"<option value="{}" selected>{}</option>"#,
                    self.get_url(index),
                    index
                ));
            } else {
                bar_content.push_str(&format!(
                    r#"<option value="{}">{}</option>"#,
                    self.get_url(index),
                    index
                ));
            }
            index += 1;
        }
        bar_content.push_str("</select>");
        bar_content
    }

    /// Returns the page content for a predefined mode (1-4).
    pub fn get_content(&mut self, mode: i32) -> String {
        match mode {
            1 => {
                self.next_page_tag = "下一页".to_owned();
                self.prev_page_tag = "上一页".to_owned();
                format!(
                    r#"{} <span class="current">{}</span> {}"#,
                    self.prev_page(),
                    self.current_page,
                    self.next_page()
                )
            }
            2 => {
                self.next_page_tag = "下一页>>".to_owned();
                self.prev_page_tag = "<<上一页".to_owned();
                self.first_page_tag = "首页".to_owned();
                self.last_page_tag = "尾页".to_owned();
                format!(
                    r#"{}{}<span class="current">[第 {} 页]</span>{}{}第{}页"#,
                    self.first_page(),
                    self.prev_page(),
                    self.current_page,
                    self.next_page(),
                    self.last_page(),
                    self.select_bar()
                )
            }
            3 => {
                self.next_page_tag = "下一页".to_owned();
                self.prev_page_tag = "上一页".to_owned();
                self.first_page_tag = "首页".to_owned();
                self.last_page_tag = "尾页".to_owned();
                let mut page_str = self.first_page();
                page_str.push_str(&self.prev_page());
                page_str.push_str(&self.page_bar());
                page_str.push_str(&self.next_page());
                page_str.push_str(&self.last_page());
                page_str.push_str(&format!(
                    r#"<span>当前页 {}/{}</span> <span>共 {} 条</span>"#,
                    self.current_page, self.total_page, self.total_size
                ));
                page_str
            }
            4 => {
                self.next_page_tag = "下一页".to_owned();
                self.prev_page_tag = "上一页".to_owned();
                self.first_page_tag = "首页".to_owned();
                self.last_page_tag = "尾页".to_owned();
                let mut page_str = self.first_page();
                page_str.push_str(&self.prev_page());
                page_str.push_str(&self.page_bar());
                page_str.push_str(&self.next_page());
                page_str.push_str(&self.last_page());
                page_str
            }
            _ => String::new(),
        }
    }

    /// Parses the URL template with the given page number and returns the URL.
    pub fn get_url(&self, page: i64) -> String {
        html_escape(
            &self
                .url_template
                .replace(DEFAULT_PAGE_PLACE_HOLDER, &page.to_string()),
        )
    }

    /// Returns the HTML `a` tag content for the given page number.
    pub fn get_link(&self, page: i64, text: &str, title: &str) -> String {
        let escaped_title = html_escape(title);
        let escaped_text = html_escape(text);
        if !self.ajax_action_name.is_empty() {
            format!(
                r#"<a class="{}" href="javascript:{}('{}')" title="{}">{}</a>"#,
                self.link_style,
                self.ajax_action_name,
                self.get_url(page),
                escaped_title,
                escaped_text
            )
        } else {
            format!(
                r#"<a class="{}" href="{}" title="{}">{}</a>"#,
                self.link_style,
                self.get_url(page),
                escaped_title,
                escaped_text
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Translated from Test_New.
    #[test]
    fn test_new() {
        let page = Page::new(9, 2, 1, "/user/list?page={.page}");
        assert_eq!(page.total_size, 9);
        assert_eq!(page.total_page, 5);
        assert_eq!(page.current_page, 1);
        let page = Page::new(9, 2, 0, "/user/list?page={.page}");
        assert_eq!(page.total_size, 9);
        assert_eq!(page.total_page, 5);
        assert_eq!(page.current_page, 1);
    }

    // Translated from Test_Basic.
    #[test]
    fn test_basic() {
        let page = Page::new(9, 2, 1, "/user/list?page={.page}");
        assert_eq!(
            page.next_page(),
            r#"<a class="GPageLink" href="/user/list?page=2" title="">&gt;</a>"#
        );
        assert_eq!(page.prev_page(), r#"<span class="GPageSpan"><</span>"#);
        assert_eq!(page.first_page(), r#"<span class="GPageSpan">|<</span>"#);
        assert_eq!(
            page.last_page(),
            r#"<a class="GPageLink" href="/user/list?page=5" title="">&gt;|</a>"#
        );
        assert_eq!(
            page.page_bar(),
            r#"<span class="GPageSpan">1</span><a class="GPageLink" href="/user/list?page=2" title="2">2</a><a class="GPageLink" href="/user/list?page=3" title="3">3</a><a class="GPageLink" href="/user/list?page=4" title="4">4</a><a class="GPageLink" href="/user/list?page=5" title="5">5</a>"#
        );

        let page = Page::new(9, 2, 3, "/user/list?page={.page}");
        assert_eq!(
            page.next_page(),
            r#"<a class="GPageLink" href="/user/list?page=4" title="">&gt;</a>"#
        );
        assert_eq!(
            page.prev_page(),
            r#"<a class="GPageLink" href="/user/list?page=2" title="">&lt;</a>"#
        );
        assert_eq!(
            page.first_page(),
            r#"<a class="GPageLink" href="/user/list?page=1" title="">|&lt;</a>"#
        );
        assert_eq!(
            page.last_page(),
            r#"<a class="GPageLink" href="/user/list?page=5" title="">&gt;|</a>"#
        );
        assert_eq!(
            page.page_bar(),
            r#"<a class="GPageLink" href="/user/list?page=1" title="1">1</a><a class="GPageLink" href="/user/list?page=2" title="2">2</a><span class="GPageSpan">3</span><a class="GPageLink" href="/user/list?page=4" title="4">4</a><a class="GPageLink" href="/user/list?page=5" title="5">5</a>"#
        );

        let page = Page::new(9, 2, 5, "/user/list?page={.page}");
        assert_eq!(page.next_page(), r#"<span class="GPageSpan">></span>"#);
        assert_eq!(
            page.prev_page(),
            r#"<a class="GPageLink" href="/user/list?page=4" title="">&lt;</a>"#
        );
        assert_eq!(
            page.first_page(),
            r#"<a class="GPageLink" href="/user/list?page=1" title="">|&lt;</a>"#
        );
        assert_eq!(page.last_page(), r#"<span class="GPageSpan">>|</span>"#);
        assert_eq!(
            page.page_bar(),
            r#"<a class="GPageLink" href="/user/list?page=1" title="1">1</a><a class="GPageLink" href="/user/list?page=2" title="2">2</a><a class="GPageLink" href="/user/list?page=3" title="3">3</a><a class="GPageLink" href="/user/list?page=4" title="4">4</a><span class="GPageSpan">5</span>"#
        );
    }

    // Translated from Test_CustomTag.
    #[test]
    fn test_custom_tag() {
        let mut page = Page::new(5, 1, 2, "/user/list/{.page}");
        page.prev_page_tag = "《".to_owned();
        page.next_page_tag = "》".to_owned();
        page.first_page_tag = "|《".to_owned();
        page.last_page_tag = "》|".to_owned();
        page.prev_bar_tag = "《《".to_owned();
        page.next_bar_tag = "》》".to_owned();
        assert_eq!(
            page.next_page(),
            r#"<a class="GPageLink" href="/user/list/3" title="">》</a>"#
        );
        assert_eq!(
            page.prev_page(),
            r#"<a class="GPageLink" href="/user/list/1" title="">《</a>"#
        );
        assert_eq!(
            page.first_page(),
            r#"<a class="GPageLink" href="/user/list/1" title="">|《</a>"#
        );
        assert_eq!(
            page.last_page(),
            r#"<a class="GPageLink" href="/user/list/5" title="">》|</a>"#
        );
        assert_eq!(
            page.page_bar(),
            r#"<a class="GPageLink" href="/user/list/1" title="1">1</a><span class="GPageSpan">2</span><a class="GPageLink" href="/user/list/3" title="3">3</a><a class="GPageLink" href="/user/list/4" title="4">4</a><a class="GPageLink" href="/user/list/5" title="5">5</a>"#
        );
    }

    // Translated from Test_CustomStyle.
    #[test]
    fn test_custom_style() {
        let mut page = Page::new(5, 1, 2, "/user/list/{.page}");
        page.link_style = "MyPageLink".to_owned();
        page.span_style = "MyPageSpan".to_owned();
        page.select_style = "MyPageSelect".to_owned();
        assert_eq!(
            page.next_page(),
            r#"<a class="MyPageLink" href="/user/list/3" title="">&gt;</a>"#
        );
        assert_eq!(
            page.prev_page(),
            r#"<a class="MyPageLink" href="/user/list/1" title="">&lt;</a>"#
        );
        assert_eq!(
            page.page_bar(),
            r#"<a class="MyPageLink" href="/user/list/1" title="1">1</a><span class="MyPageSpan">2</span><a class="MyPageLink" href="/user/list/3" title="3">3</a><a class="MyPageLink" href="/user/list/4" title="4">4</a><a class="MyPageLink" href="/user/list/5" title="5">5</a>"#
        );
        assert_eq!(
            page.select_bar(),
            r#"<select name="MyPageSelect" onchange="window.location.href=this.value"><option value="/user/list/1">1</option><option value="/user/list/2" selected>2</option><option value="/user/list/3">3</option><option value="/user/list/4">4</option><option value="/user/list/5">5</option></select>"#
        );
    }

    // Translated from Test_Ajax.
    #[test]
    fn test_ajax() {
        let mut page = Page::new(5, 1, 2, "/user/list/{.page}");
        page.ajax_action_name = "LoadPage".to_owned();
        assert_eq!(
            page.next_page(),
            r#"<a class="GPageLink" href="javascript:LoadPage('/user/list/3')" title="">&gt;</a>"#
        );
        assert_eq!(
            page.prev_page(),
            r#"<a class="GPageLink" href="javascript:LoadPage('/user/list/1')" title="">&lt;</a>"#
        );
        assert_eq!(
            page.page_bar(),
            r#"<a class="GPageLink" href="javascript:LoadPage('/user/list/1')" title="1">1</a><span class="GPageSpan">2</span><a class="GPageLink" href="javascript:LoadPage('/user/list/3')" title="3">3</a><a class="GPageLink" href="javascript:LoadPage('/user/list/4')" title="4">4</a><a class="GPageLink" href="javascript:LoadPage('/user/list/5')" title="5">5</a>"#
        );
    }

    // Translated from Test_PredefinedContent.
    #[test]
    fn test_predefined_content() {
        let mut page = Page::new(5, 1, 2, "/user/list/{.page}");
        page.ajax_action_name = "LoadPage".to_owned();
        assert_eq!(
            page.get_content(1),
            r#"<a class="GPageLink" href="javascript:LoadPage('/user/list/1')" title="">上一页</a> <span class="current">2</span> <a class="GPageLink" href="javascript:LoadPage('/user/list/3')" title="">下一页</a>"#
        );
        assert_eq!(
            page.get_content(2),
            r#"<a class="GPageLink" href="javascript:LoadPage('/user/list/1')" title="">首页</a><a class="GPageLink" href="javascript:LoadPage('/user/list/1')" title="">&lt;&lt;上一页</a><span class="current">[第 2 页]</span><a class="GPageLink" href="javascript:LoadPage('/user/list/3')" title="">下一页&gt;&gt;</a><a class="GPageLink" href="javascript:LoadPage('/user/list/5')" title="">尾页</a>第<select name="GPageSelect" onchange="window.location.href=this.value"><option value="/user/list/1">1</option><option value="/user/list/2" selected>2</option><option value="/user/list/3">3</option><option value="/user/list/4">4</option><option value="/user/list/5">5</option></select>页"#
        );
        assert_eq!(
            page.get_content(3),
            r#"<a class="GPageLink" href="javascript:LoadPage('/user/list/1')" title="">首页</a><a class="GPageLink" href="javascript:LoadPage('/user/list/1')" title="">上一页</a><a class="GPageLink" href="javascript:LoadPage('/user/list/1')" title="1">1</a><span class="GPageSpan">2</span><a class="GPageLink" href="javascript:LoadPage('/user/list/3')" title="3">3</a><a class="GPageLink" href="javascript:LoadPage('/user/list/4')" title="4">4</a><a class="GPageLink" href="javascript:LoadPage('/user/list/5')" title="5">5</a><a class="GPageLink" href="javascript:LoadPage('/user/list/3')" title="">下一页</a><a class="GPageLink" href="javascript:LoadPage('/user/list/5')" title="">尾页</a><span>当前页 2/5</span> <span>共 5 条</span>"#
        );
        assert_eq!(
            page.get_content(4),
            r#"<a class="GPageLink" href="javascript:LoadPage('/user/list/1')" title="">首页</a><a class="GPageLink" href="javascript:LoadPage('/user/list/1')" title="">上一页</a><a class="GPageLink" href="javascript:LoadPage('/user/list/1')" title="1">1</a><span class="GPageSpan">2</span><a class="GPageLink" href="javascript:LoadPage('/user/list/3')" title="3">3</a><a class="GPageLink" href="javascript:LoadPage('/user/list/4')" title="4">4</a><a class="GPageLink" href="javascript:LoadPage('/user/list/5')" title="5">5</a><a class="GPageLink" href="javascript:LoadPage('/user/list/3')" title="">下一页</a><a class="GPageLink" href="javascript:LoadPage('/user/list/5')" title="">尾页</a>"#
        );
        assert_eq!(page.get_content(5), "");
    }

    #[test]
    fn html_escaping_matches_go() {
        assert_eq!(html_escape(r#"&<>"'"#), "&amp;&lt;&gt;&#34;&#39;");
    }
}
