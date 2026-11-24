//! Benchmarks shared with the Go counterpart in `bench/go`.

use std::backtrace::Backtrace;
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use gf_container::{
    garray::{Array, SortedArray, SortedIntArray},
    glist::List,
    gmap::{ListKVMap, Map, TreeMap},
    gqueue::Queue,
    gring::Ring,
    gset::Set,
    gtree::{RedBlackTree, TreeComparator},
    gtype,
    gvar::Var,
};
use gf_core::Value;
use gf_crypto::{gaes, gcrc32, gmd5, gsha256};
use gf_debug::gdebug;
use gf_encoding::{
    gbase64, gbinary, gcharset, gcompress, ghash, ghtml, gjson::Json, gproperties, gtoml, gurl,
    gxml, gyaml,
};
use gf_errors::gcode;
use gf_errors::gerror;
use gf_internal::{command, empty, utils};
use gf_net::gipv4;
use gf_os::genv;
use gf_os::gtime;
use gf_text::gregex;
use gf_text::gstr;
use gf_util::{gconv, gpage, grand, guid};
use indexmap::IndexMap;

fn report<T>(name: &str, iters: u64, mut run: impl FnMut() -> T) {
    for _ in 0..(iters / 10).max(1) {
        black_box(run());
    }
    let mut best = f64::MAX;
    for _ in 0..5 {
        let start = Instant::now();
        for _ in 0..iters {
            black_box(run());
        }
        let ns = start.elapsed().as_nanos() as f64 / iters as f64;
        best = best.min(ns);
    }
    println!("{name}\t{iters}\t{best:.1}");
}

fn main() {
    let buffer = vec![0x5au8; 1024];

    report("ghash_16x1KiB", 50_000, || {
        let mut acc = 0u64;
        acc ^= u64::from(ghash::ap(&buffer));
        acc ^= ghash::ap64(&buffer);
        acc ^= u64::from(ghash::bkdr(&buffer));
        acc ^= ghash::bkdr64(&buffer);
        acc ^= u64::from(ghash::djb(&buffer));
        acc ^= ghash::djb64(&buffer);
        acc ^= u64::from(ghash::elf(&buffer));
        acc ^= ghash::elf64(&buffer);
        acc ^= u64::from(ghash::js(&buffer));
        acc ^= ghash::js64(&buffer);
        acc ^= u64::from(ghash::pjw(&buffer));
        acc ^= ghash::pjw64(&buffer);
        acc ^= u64::from(ghash::rs(&buffer));
        acc ^= ghash::rs64(&buffer);
        acc ^= u64::from(ghash::sdbm(&buffer));
        acc ^= ghash::sdbm64(&buffer);
        acc
    });

    let payload = vec![0xA5u8; 1024];
    report("base64_encode_1KiB", 200_000, || {
        gbase64::encode_to_string(&payload)
    });

    const URL: &str = "https://golang.org/x/crypto?go-get=1 +";
    report("gurl_encode", 500_000, || gurl::encode(URL));
    report("gurl_parse", 500_000, || gurl::parse_url(URL, -1));

    let mut json_map = IndexMap::new();
    for i in 0..8u32 {
        json_map.insert(
            format!("key{i}"),
            if i % 2 == 0 {
                Value::String(format!("value{i}"))
            } else {
                Value::Int(i64::from(i))
            },
        );
    }
    let json_value = Value::Map(json_map);
    report("json_marshal_map", 200_000, || {
        gf_json::marshal_value(&json_value)
    });

    const JSON_BYTES: &[u8] = br#"{"key0":"value0","key1":1,"key2":"value2","key3":3,"key4":"value4","key5":5,"key6":"value6","key7":7}"#;
    report("json_unmarshal_map", 200_000, || {
        gf_json::unmarshal_value(JSON_BYTES)
    });

    report("md5_1KiB", 200_000, || gmd5::encrypt_bytes(&payload));
    report("sha256_1KiB", 200_000, || gsha256::encrypt_bytes(&payload));

    const HTML: &str =
        r##"<p>Test paragraph.</p><!-- Comment -->  <a href="#fragment">Other text</a>"##;
    report("ghtml_strip_tags", 200_000, || ghtml::strip_tags(HTML));
    report("ghtml_entities", 200_000, || {
        ghtml::entities(r#"A 'quote' "is" <b>bold</b>"#)
    });

    const TOML_DOC: &str = "viewpath = \"/home/www/templates/\"\n[redis]\ndisk = \"127.0.0.1:6379,0\"\ncache = \"127.0.0.1:6379,1\"\n";
    report("gtoml_decode", 100_000, || {
        gtoml::decode(TOML_DOC.as_bytes())
    });
    const YAML_DOC: &str = "url: https://goframe.org\nserver:\n  - 120.168.117.21\n  - 120.168.117.22\npi: 3.14\nhasChild: true\nname: '你好YAML'\n";
    report("gyaml_decode", 100_000, || {
        gyaml::decode(YAML_DOC.as_bytes())
    });

    report("gipv4_intranet", 1_000_000, || {
        gipv4::is_intranet("192.168.1.1")
    });
    report("gregex_match", 200_000, || {
        gregex::is_match_string(r"(.+):(\d+)", "sfs:2323")
    });

    report("gcrc32_1KiB", 200_000, || gcrc32::encrypt(&payload));
    report("grand_intn", 2_000_000, || grand::intn(1000));
    report("gbinary_encode", 500_000, || {
        gbinary::encode(&[Value::Int(123), Value::UInt(456), Value::String("x".into())])
    });
    const PROPERTIES: &[u8] = b"viewpath = \"/home/www/templates/\"\nredis.disk = \"127.0.0.1:6379,0\"\nredis.cache = \"127.0.0.1:6379,1\"\n";
    report("gproperties_decode", 200_000, || {
        gproperties::decode(PROPERTIES)
    });
    let aes_key = b"1234567891234567";
    report("gaes_cbc_encrypt", 100_000, || {
        gaes::encrypt_cbc(b"pibigstar", aes_key, None)
    });
    let aes_cipher_text = gaes::encrypt_cbc(b"pibigstar", aes_key, None).expect("encrypt");
    report("gaes_cbc_decrypt", 100_000, || {
        gaes::decrypt_cbc(&aes_cipher_text, aes_key, None)
    });

    const XML: &str = "<root><bool>true</bool><float>100.92</float><int>123</int><string>hello world</string></root>";
    report("gxml_decode", 200_000, || gxml::decode(XML.as_bytes()));
    let xml_value = gxml::decode(XML.as_bytes()).expect("xml");
    report("gxml_encode", 200_000, || gxml::encode(&xml_value, None));

    report("gzip_1KiB", 100_000, || gcompress::gzip(&payload, None));
    let gzipped = gcompress::gzip(&payload, None).expect("gzip");
    report("gunzip_1KiB", 100_000, || gcompress::un_gzip(&gzipped));

    const GBK_BYTES: &[u8] = &[
        0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0xb3, 0xa3, 0xd3, 0xc3, 0x87, 0xf8, 0xd7, 0xd6, 0x98,
        0xcb, 0x9c, 0xca, 0xd7, 0xd6, 0xf3, 0x77, 0xb1, 0xed,
    ];
    report("gcharset_gbk_to_utf8", 200_000, || {
        gcharset::to_utf8("gbk", GBK_BYTES)
    });

    const GJSON: &str =
        r#"{"users":[{"name":"john","score":90},{"name":"jane","score":80}],"meta":{"count":2}}"#;
    let conv_value = Value::String("123456".into());
    report("gconv_int", 1_000_000, || gconv::int64_value(&conv_value));
    let json_doc = Json::new_from(GJSON.as_bytes()).expect("json");
    report("gjson_get", 1_000_000, || {
        json_doc.get_string("users.0.name")
    });
    report("gjson_new", 200_000, || Json::new_from(GJSON.as_bytes()));

    report("gstr_len_rune", 2_000_000, || {
        gstr::len_rune("Hello 常用國字標準字體表")
    });
    report("gstr_contains_i", 1_000_000, || {
        gstr::contains_i("GoFrame Framework", "framework")
    });
    report("gstr_split_join", 500_000, || {
        gstr::join(&gstr::split("a,b,c,d,e", ","), "-")
    });
    report("gstr_is_sub_domain", 1_000_000, || {
        gstr::is_sub_domain("s.goframe.org", "goframe.org")
    });
    report("gstr_uc_words", 500_000, || {
        gstr::uc_words("hello world from goframe")
    });

    report("gstr_case_snake", 500_000, || {
        gstr::case_snake("AnyKindOfString")
    });
    report("gstr_number_format", 500_000, || {
        gstr::number_format(1234.56, 2, ",", " ")
    });
    report("gstr_word_wrap", 300_000, || {
        gstr::word_wrap("hello world from goframe", 10, "\n")
    });
    report("gstr_parse", 300_000, || gstr::parse("v[a]=m&v[b]=n"));
    report("gstr_compare_version", 1_000_000, || {
        gstr::compare_version("v1.0.1", "v1.0.2")
    });
    report("gstr_similar_text", 200_000, || {
        gstr::similar_text("Hello", "World")
    });

    report("gvar_int", 1_000_000, || {
        let var = Var::new(Value::Int(123));
        var.int64()
    });
    report("gmap_set_get", 300_000, || {
        let map: Map<String, i64> = Map::new();
        map.set("k".to_owned(), 1);
        map.get(&"k".to_owned())
    });
    report("gdebug_caller", 10_000, || gdebug::caller(0));

    report("garray_append_at", 300_000, || {
        let array: Array<i64> = Array::new();
        array.append([1, 2, 3]);
        array.at(2)
    });
    report("gset_add_contains", 300_000, || {
        let set: Set<i64> = Set::new();
        set.add([1, 2, 3]);
        set.contains(&2)
    });
    report("glist_push_pop", 500_000, || {
        let list: List<i64> = List::new();
        list.push_back(1);
        list.pop_front()
    });
    report("gring_put_slice", 300_000, || {
        let ring: Ring<i64> = Ring::new(4);
        ring.put(1);
        ring.put(2);
        ring.slice_next()
    });
    let bench_queue: Queue<i64> = Queue::new(None);
    report("gqueue_push_pop", 300_000, || {
        bench_queue.push(1);
        bench_queue.pop()
    });
    bench_queue.close();

    report("backtrace_capture", 100_000, Backtrace::force_capture);

    report("gerror_new_1", 200_000, || gerror::Error::new("base"));

    report("gerror_new_no_stack", 200_000, || {
        gerror::new_with_option(gerror::ErrorOption {
            error: None,
            stack: false,
            text: "base".into(),
            args: Vec::new(),
            code: gcode::CODE_NIL.clone(),
        })
    });

    report("gerror_create_chain3", 100_000, || {
        let inner = gerror::Error::new("base");
        let mid = gerror::wrap(Some(inner), "mid").expect("wrap");
        gerror::wrap(Some(mid), "top").expect("wrap")
    });

    let chain = {
        let inner = gerror::Error::new("base");
        let mid = gerror::wrap(Some(inner), "mid").expect("wrap");
        gerror::wrap(Some(mid), "top").expect("wrap")
    };
    report("gerror_render_chain3", 500_000, || chain.error_string());

    report("gerror_newf_format", 200_000, || {
        gerror::newf(
            "value=%s count=%d",
            vec![Value::String("value".into()), Value::Int(42)],
        )
        .error_string()
    });

    let mut replaces = IndexMap::new();
    replaces.insert("_".to_owned(), "-".to_owned());
    report("utils_strings", 200_000, || {
        let removed = utils::remove_symbols("my_custom_key-42 value");
        let numeric = utils::is_numeric("123.45");
        let trimmed = utils::trim("---my_custom_key---", Some("-"));
        let formatted = utils::format_cmd_key("MY_CUSTOM_KEY");
        (removed, numeric, trimmed, formatted)
    });

    let command_args: Vec<String> = ["gf", "gen", "-n", "2", "--name=x", "-d", "-h"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    report("command_parse", 200_000, || {
        command::parse_using_default_algorithm(&command_args)
    });

    let empty_map = Value::Map(IndexMap::new());
    let mut filled_map = IndexMap::new();
    filled_map.insert("k".to_owned(), Value::Int(1));
    let non_empty_map = Value::Map(filled_map);
    report("empty_checks", 2_000_000, || {
        let mut hits = 0u32;
        hits += u32::from(empty::is_empty(&Value::Int(0)));
        hits += u32::from(empty::is_empty(&Value::String(String::new())));
        hits += u32::from(empty::is_empty(&empty_map));
        hits += u32::from(empty::is_empty(&non_empty_map));
        hits
    });

    report("gtype_set_val_add_cas", 1_000_000, || {
        let value = gtype::Int::new(Some(0));
        value.set(1);
        value.add(1);
        value.cas(2, 3);
        value.val()
    });
    report("gtype_any_set_val", 1_000_000, || {
        let value = gtype::Interface::new(Some(Value::Int(123)));
        value.set(Value::Int(1));
        value.val()
    });

    report("gsorted_int_add_search", 300_000, || {
        let array: SortedIntArray = SortedArray::new(None);
        array.add([1, 2, 3]);
        array.search(&2)
    });

    report("gmap_list_set_get", 300_000, || {
        let map: ListKVMap<String, Value> = ListKVMap::new();
        map.set("k".to_owned(), Value::Int(1));
        map.get(&"k".to_owned())
    });
    let iterate_list: ListKVMap<String, Value> = ListKVMap::new();
    for index in 0..100 {
        iterate_list.set(format!("k{index}"), Value::Int(index));
    }
    report("gmap_list_iterate_100", 100_000, || {
        let mut count = 0;
        iterate_list.iterator(|_, _| {
            count += 1;
            true
        });
        count
    });
    let tree_comparator: TreeComparator<Value> = Arc::new(gf_util::gutil::comparator_string);
    report("gmap_tree_set_get", 300_000, || {
        let map = TreeMap::new(Some(Arc::clone(&tree_comparator)));
        map.set("k".into(), Value::Int(1));
        map.get(&"k".into())
    });
    report("gtree_set_get", 300_000, || {
        let tree = RedBlackTree::new(Some(Arc::clone(&tree_comparator)));
        tree.set("k".into(), Value::Int(1));
        tree.get(&"k".into())
    });
    let int_comparator: TreeComparator<Value> = Arc::new(gf_util::gutil::comparator_int);
    let iterate_tree = RedBlackTree::new(Some(int_comparator));
    for index in 0..100 {
        iterate_tree.set(Value::Int(index), Value::Int(index));
    }
    report("gtree_iterate_100", 200_000, || {
        let mut count = 0;
        iterate_tree.iterator(|_, _| {
            count += 1;
            true
        });
        count
    });

    report("gpage_page_bar", 500_000, || {
        let page = gpage::Page::new(100, 10, 5, "/user/list?page={.page}");
        page.page_bar()
    });
    report("guid_s", 200_000, || guid::s(&[]));

    let env_entries = ["a=1".to_owned(), "b=2".to_owned()];
    report("genv_map_from_env", 500_000, || {
        genv::map_from_env(&env_entries)
    });

    let bench_time = gtime::Time::new_from_time_stamp(1_800_000_000);
    report("gtime_format", 1_000_000, || {
        bench_time.format("Y-m-d H:i:s")
    });
    report("gtime_str_to_time", 200_000, || {
        gtime::str_to_time("2018-07-18 12:01:02", None)
    });
}
