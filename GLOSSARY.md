# Glossary

Every original term, its meaning, and its name in Rust. Names are fixed: units
that bring a new term add it here and reuse the same name everywhere.

| Go | Meaning | Rust |
| --- | --- | --- |
| `github.com/gogf/gf/v2` | the framework module | facade crate `gf` |
| `any` | dynamic value passed through APIs | `gf_core::Value` |
| `map[string]any` | string-keyed dynamic map | `Value::Map(IndexMap<String, Value>)` |
| `[]byte` | byte slice | `Vec<u8>` / `&[u8]` |
| `nil` | absent value / absent error detail | `Value::Null` / `None` |
| `error` | error value | `Result<T, GError>` |
| `gcode.Code` | error code interface | trait `gf_errors::gcode::Code` |
| `gerror.New` / `Newf` | errors with text and stack | `gf_errors::gerror::{new, newf}` |
| `gerror.NewCode` / `NewCodef` | errors with a code | `new_code` / `new_codef` (texts join with `", "`) |
| `gerror.Wrap` / `Wrapf` | wrap inheriting the code | `wrap(Option<Error>, text)` / `wrapf` |
| `gerror.WrapCode` / `WrapCodef` | wrap with an explicit code | `wrap_code` / `wrap_codef` |
| `gerror.Option` | creation options | `gerror::ErrorOption` + `new_with_option` |
| `gerror.Cause` / `Current` / `Unwrap` | chain navigation | `cause` / `current` / `unwrap` |
| `gerror.Code` / `HasCode` | effective code lookup | `code(&Error)` / `has_code` |
| `gerror.Equal` / `Is` | error comparison | `equal` / `is` (code + current text) |
| `gerror.Stack` / `HasStack` | rendered stack | `stack(&Error)` / `has_stack` |
| `gerror.Error.TextWithArgs` | deferred Go `fmt` formatting | `text_with_args`, `gfmt::sprintf` |
| `fmt` on errors (`%+v`, `%-s`) | format flags | `verbose()` / `current()` / `stack()` methods |
| `gerror.Error.MarshalJSON` | JSON string of the error | `marshal_json` |
| `localCode` | concrete error code | `gf_errors::gcode::LocalCode` |
| `gcode.CodeNil` ... | reserved codes | `gcode::CODE_NIL` ... |
| `StackMode` / `StackModeBrief` / `StackModeDetail` | error stack detail level | `gf_internal::errors::StackMode::{Brief, Detail}` |
| `command.ParseUsingDefaultAlgorithm` | argv/option parser | `gf_internal::command::parse_using_default_algorithm` |
| `command.GetOptWithEnv` | option with env fallback | `gf_internal::command::get_opt_with_env` |
| `ghash.BKDR` / `BKDR64` | BKDR hash | `gf_encoding::ghash::bkdr` / `bkdr64` |
| `ghash.AP` / `AP64` | AP hash | `ap` / `ap64` |
| `ghash.DJB` / `DJB64` | DJB hash | `djb` / `djb64` |
| `ghash.ELF` / `ELF64` | ELF hash | `elf` / `elf64` |
| `ghash.JS` / `JS64` | JS hash | `js` / `js64` |
| `ghash.PJW` / `PJW64` | PJW hash | `pjw` / `pjw64` |
| `ghash.RS` / `RS64` | RS hash | `rs` / `rs64` |
| `ghash.SDBM` / `SDBM64` | SDBM hash | `sdbm` / `sdbm64` |
| `gbase64.Encode` / `EncodeToString` | BASE64 encoding | `gbase64::{encode, encode_to_string}` |
| `gbase64.Decode` / `DecodeString` | BASE64 decoding | `gbase64::{decode, decode_string}` (plus `*_bytes` forms) |
| `gbase64.EncodeFile` / `DecodeToString` | file and text helpers | `encode_file`, `decode_to_string`, `must_*` variants |
| `gurl.Encode` / `RawEncode` | query / RFC 3986 escaping | `gurl::{encode, raw_encode}` |
| `gurl.Decode` / `RawDecode` | percent decoding | `gurl::{decode, raw_decode}` (plus `*_bytes`) |
| `gurl.BuildQuery` | encoded query string | `build_query(&BTreeMap<String, Vec<String>>)` |
| `gurl.ParseURL` | URL components | `parse_url(&str, component)` → `HashMap<&'static str, Cow<str>>` |
| `json.Marshal` / `MarshalIndent` | JSON encoding | `gf_json::{marshal, marshal_string, marshal_indent}` |
| `json.Unmarshal` / `Valid` | JSON decoding | `gf_json::{unmarshal, unmarshal_value, valid}` |
| `json.RawMessage` | raw JSON text | `gf_json::RawMessage` (`Box<RawValue>`) |
| `json` floats | Go g-float text | `marshal_value`/`marshal_indent` write `1e+300`/`1e-07` via `gf_core::float::format_g` (ADR-095) |
| `ghtml.StripTags` | remove markup, keep text | `ghtml::strip_tags` |
| `ghtml.Entities` / `SpecialChars` | escape five characters | `entities` / `special_chars` |
| `ghtml.EntitiesDecode` / `SpecialCharsDecode` | decode entities | `entities_decode` / `special_chars_decode` |
| `ghtml.SpecialCharsMapOrStruct` | escape map string values | `special_chars_map` / `special_chars_map_or_struct` |
| `gtoml.Encode` / `Decode` / `DecodeTo` / `ToJson` | TOML access | `gtoml::{encode, decode, decode_to, to_json}` |
| `gyaml.Encode` / `EncodeIndent` / `Decode` / `DecodeTo` / `ToJson` | YAML access | `gyaml::{encode, encode_indent, decode, decode_to, to_json}` |
| `gregex.*` | cached regexp helpers | same names in `gf_text::gregex` (`match_string` returns `Option<Vec<String>>`) |
| `gtag.Set/Get/Parse` | tag store | `gf_util::gtag::{set, get, parse}` (also `set_over`, `sets`) |
| `gtag.SetGlobalEnums` / `GetEnumsByType` | enum descriptions | `set_global_enums` / `get_enums_by_type` |
| `gipv4.IpToLong*` / `LongToIp*` / `Validate` / `IsIntranet` | IPv4 helpers | same names in `gf_net::gipv4` |
| `gipv4.GetIntranetIp` / `GetIpArray` / `GetMac` / `GetHostByName` | host inspection | `get_intranet_ip`, `get_ip_array`, `get_mac`, `get_host_by_name` |
| `gmd5.Encrypt*` | MD5 helpers | `gmd5::{encrypt_bytes, encrypt_string, encrypt_file}` |
| `gsha1/gsha256/gsha512.Encrypt` | SHA helpers | `encrypt_bytes` / `encrypt_string` / `encrypt_file` |
| `Encrypt(any)` (hashes) | `gconv.Bytes` conversion | waits for `gconv` (ADR-017) |
| `gcrc32.Encrypt` | IEEE CRC32 | `gcrc32::encrypt(&[u8])` / `encrypt_string` |
| `gaes.Encrypt/Decrypt` | AES-CBC with PKCS#7 | `gaes::{encrypt_cbc, decrypt_cbc}` (`iv: Option<&[u8]>`) |
| `gaes.EncryptCFB/DecryptCFB` | AES-CFB with zero padding | `gaes::{encrypt_cfb, decrypt_cfb}` (`padding: &mut usize`) |
| `gaes.PKCS5/7Padding` | padding helpers | `pkcs5_padding`, `pkcs7_padding` (+ unpadding) |
| `gdes.EncryptECB/CBC*` | DES and 3DES modes | `gdes::{encrypt_ecb, decrypt_ecb, encrypt_cbc, ...}` with `NOPADDING`/`PKCS5PADDING` |
| `gbinary.Encode/Decode` | LE binary encoding | `gbinary::{encode, decode}` (`Value` + `Kind`), BE via `be_*` |
| `gbinary.Bit` helpers | bit packing | `encode_bits`, `encode_bits_to_bytes`, `decode_bits`, `decode_bytes_to_bits` |
| `gproperties.Decode/Encode/ToJson` | Java properties | `gproperties::{decode, encode, to_json}` (references, cycles) |
| `grand.Intn/B/S/N/Str/...` | random helpers | same names in `gf_util::grand` (`D` takes `Duration`) |
| `gipv6.Validate` | IPv6 pattern | `gf_net::gipv6::validate` |
| `gcharset.Convert` | charset conversion | `gcharset::{convert, convert_bytes}` (bytes-first, ADR-023) |
| `gcharset.ToUTF8` / `UTF8To` | direction helpers | `to_utf8` / `utf8_to` |
| `gcharset.Supported` | label check | `supported` |
| `gxml.Decode` / `DecodeWithoutRoot` | XML to map | `gxml::{decode, decode_without_root}` |
| `gxml.Encode` / `EncodeWithIndent` | map to XML | `gxml::{encode, encode_with_indent}` |
| `gxml.XMLEscapeChars` | global escape switch | `gxml::xml_escape_chars(bool)` |
| `gcompress.Gzip` / `UnGzip` | gzip data | `gcompress::{gzip, un_gzip}` (`level: Option<u32>`) |
| `gcompress.Zlib` / `UnZlib` | zlib data | `gcompress::{zlib, un_zlib}` (short input passthrough) |
| `gcompress.GzipFile` / `UnGzipFile` | gzip files | `gzip_file` / `un_gzip_file` |
| `grsa.Encrypt` / `Decrypt` | PKCS#1 v1.5 | `grsa::{encrypt, decrypt}` plus `*_pkcs1`/`*_pkcs8` |
| `grsa.EncryptOAEP` / `DecryptOAEP` | OAEP SHA-256 | `grsa::{encrypt_oaep, decrypt_oaep}` (no labels, ADR-025) |
| `grsa.GenerateKeyPair*` | key generation | `generate_key_pair` / `generate_key_pair_pkcs8` |
| `grsa.GetPrivateKeyType` | PKCS#1 vs PKCS#8 | `get_private_key_type` (`KEY_TYPE_PKCS1`/`KEY_TYPE_PKCS8`) |
| `grsa.ExtractPKCS1PublicKey` | public key from private | `extract_pkcs1_public_key` |
| `gcompress.ZipPath*` / `UnZip*` | zip archives | `gcompress::{zip_path, zip_path_writer, zip_path_content, un_zip_file, un_zip_content}` |
| `gini.Decode` / `Encode` / `ToJson` | INI access | `gini::{decode, encode, to_json}` |
| `gmode.Mode` / `Set*` | release mode | `gmode::{mode, set, set_develop, ...}` (`debug_assertions` detection, ADR-026) |
| `gconv.String/Bool/Int*/Uint*/Float*/Bytes` | value conversion | same names in `gf_util::gconv` (`*_value` where the Go name is a keyword) |
| `gconv.Strings/Ints/Floats/Bools/Map/Maps` | slice and map conversion | `strings`, `ints`, `floats`, `bools`, `map`, `maps` |
| `gconv.Scan` / `Struct` | struct scanning | `scan::<T>` (serde, ADR-027) / `FromValue` |
| `gutil` map/slice helpers | map/slice utilities | `Map`, `map_copy/contains/delete/merge/merge_copy/omit_empty/to_slice`, `slice_copy/insert_before/insert_after/delete`, `slice_to_map`, `slice_to_map_with_column_as_key`, `is_empty_value` (ADR-133) |
| `gutil.MapPossibleItemByKey` / `MapContainsPossibleKey` | case/symbol-insensitive map key lookup | `map_possible_item_by_key` / `map_contains_possible_key` |
| `gvalid.New` / `Validator` | validation manager | `gf_valid::gvalid::{new, Validator}`, chainable and consuming `self` |
| `Validator.Check/CheckMap/CheckValue` | validation lanes | `Validator::{run, check_map, check_value}` (`check_struct` replaced by a rules map, ADR-039) |
| `gvalid.Assoc` | associated rule data | `Validator::assoc(value)`, forwarded as the rules' `data` in the scalar lane (ADR-153) |
| `gvalid.Rules` argument (`any`) | rules text, map or tag sequence | `gvalid::Rules::{Text, Map, Sequence}` |
| `gvalid.RuleFunc` / `RuleFuncInput` | custom rules | same names in `gf_valid::gvalid` (`RuleFunc` is an `Arc<dyn Fn>`; no `reflect.Type`) |
| `gvalid.RegisterRule` / `DeleteRule` | package-level custom rules | `register_rule`, `register_rule_by_map`, `delete_rule`, `get_registered_rule_map` |
| `gvalid.Error` | validation result error | `gvalid::ValidationError` (`maps`, `map`, `items`, `first_*`, `strings`) |
| `gvalid.ParseTagValue` | `[alias@]rule[#msg]` parsing | `parse_tag_value` (groups read by position) |
| `builtin.RunInput` / `builtin.GetRule` | builtin rule table | private `gvalid::builtin::{RunInput, get_rule}` with 67 rules |
| `gi18n` messages in gvalid | translated validation messages | not ported; custom messages plus builtin English (ADR-039) |
| `gjson.New` / `NewFrom` | document creation | `gjson::Json::new` / `new_from` (format detection) |
| `gjson.Get/Set/Remove` | dotted path access | `Json::{get, set, remove}` with typed `get_*` helpers |
| `gjson.Len` / `MustTo*` | document helpers | `Json::{len, is_empty, is_nil, must_to_json, must_to_xml, must_to_yaml, must_to_toml, must_to_ini, must_to_properties}` (ADR-151) |
| `gjson.ToJson/ToYaml/ToToml/ToXml/ToProperties/ToIni` | format output | `Json::{to_json, to_yaml, to_toml, to_xml, to_properties, to_ini}` |
| `gstr` trim/prefix helpers | edge helpers | `trim_str(origin, cut, Option<usize>)`, `trim_right_str`, `has_prefix`, `has_suffix`, `nl2_br(s, Option<bool>)` (ADR-132) |
| `gstr` case conversions | case family | `CaseType`, `case_type_match`, `case_convert`, `case_camel*`, `case_snake*`, `case_kebab*`, `case_delimited*`, `case_snake_first_upper` (Go tables, ADR-131) |
| `gstr.*` (core) | string helpers | same names in `gstr` (`len_rune`, `contains_i`, `pos_i`, `is_sub_domain`, `uc_words`, ...) |
| `gstr.Split/Join` | splitting and joining | `split` returns owned pieces; `join(&[String], sep)`; `join_any(Value, sep)` |
| `gstr.Trim*` / `SplitAndTrim` | masking trims | `trim`, `trim_left`, `trim_right`, `trim_all` (mask `Option<&str>`) |
| `gstr.CountWords/CountChars` | frequency maps | `count_words` / `count_chars` (`HashMap<String, usize>`) |
| `gstr.Pos*/PosR*` with offsets | positioned search | `pos_offset`, `pos_i_offset`, `pos_r`, `pos_ri`, `pos_rune`, `pos_i_rune` |
| `gstr.Str/StrEx/StrTill*` | split around a needle | `str_part`, `str_part_ex`, `str_till`, `str_till_ex` |
| `gstr.SubStr*/StrLimit*` | substrings and truncation | `sub_str`, `sub_str_rune`, `str_limit`, `str_limit_rune`, `sub_str_from*` |
| `gstr.Replace*` | replacement family | `replace`, `replace_i`, `replace_by_array`, `replace_by_map`, `replace_func` (+ `i` variants) |
| `gstr.Case*` | naming conventions | `CaseType`, `case_convert`, `case_camel`, `case_snake`, `case_kebab`, `case_snake_first_upper` |
| `gstr.Chr/Ord/OctStr/Reverse/NumberFormat/Shuffle/HideStr/Nl2Br/WordWrap` | conversions | same names in `gstr` |
| `gstr.Parse` | ampersand form to nested map | `gstr::parse` → `Value` |
| `gstr.Levenshtein/SimilarText/Soundex` | string similarity | `levenshtein`, `similar_text` (returns `(usize, f64)`), `soundex` |
| `gstr.IsGNUVersion/CompareVersion*` | version comparison | `is_gnu_version`, `compare_version`, `compare_version_go` |
| `gdebug.Caller*` | caller information | `gdebug::{caller, caller_with_filter, caller_file_path, caller_file_line, ...}` (native symbols, ADR-029) |
| `gdebug.Stack/PrintStack` | formatted stack | `gdebug::{stack, print_stack}` (`filters: &[String]`, `skip`) |
| `gdebug.BinVersion*` | binary fingerprints | `gdebug::{bin_version, bin_version_md5}` |
| `gdebug.GoroutineID` | goroutine id | `gdebug::goroutine_id` (always 0) |
| `gvar.New` / methods | locked dynamic value | `gf_container::gvar::Var` (`new`, `set`, `val`, typed getters, JSON) |
| `gmap.New*` / `Set/Get/*` | synchronized maps | generic `gf_container::gmap::Map<K, V>` (typed aliases `StrAnyMap`, ...; ADR-030) |
| `garray` mutators | in-place array edits | `Array::{replace, fill, insert_before, insert_after, pad, reverse, shuffle}` (ADR-134) |
| `garray.*Array` | growable arrays | generic `garray::Array<T>` (aliases `AnyArray`, `StrArray`, `IntArray`) |
| `gset.*Set` | sets | generic `gset::Set<T>` (aliases `StrSet`, `IntSet`; set algebra methods) |
| `glist.*List` | double-ended lists | generic `glist::List<T>` (index-based add/remove) |
| `gring.Ring` | circular buffer | generic `gring::Ring<T>` (`put/val/move_by/next/prev/slice_next`) |
| `gqueue.Queue` | blocking queue | generic `gqueue::Queue<T>` (`push/pop/close`, optional limit) |
| `gtype.Any` / `gtype.Interface` | locked dynamic value | `gtype::Interface` (`pub type Any = Interface`; `new(Option<Value>)`) |
| `gtype.Bool/Byte/Bytes/Float32/Float64/Int/Int32/Int64/String/Uint/Uint32/Uint64` | locked scalar types | same names in `gf_container::gtype` (`set` returns the old value, `add` the new one, `cas` a bool) |
| `gtype.DeepCopy` | deep copy | `deep_copy` returning `Self` (no nil receiver) |
| `garray.SortedArray` / `SortedTArray` | comparator-sorted array | generic `garray::SortedArray<T>` (aliases `SortedAnyArray`, `SortedIntArray`, `SortedStrArray`); sorted `pop_left`/`pop_right`/`pop_lefts`/`pop_rights` pinned to Go examples (ADR-094) |
| `garray.SetSorter` / `SetComparator` | sorting hooks | `set_sorter(Option<Sorter<T>>)` / `set_comparator(Option<Comparator<T>>)` (`&mut self`) |
| `gmap.KVMap` | hash map with extras | `gmap::KVMap<K, V>` = `Map<K, V>` (adds `search`, `pop/pops`, `*WithError`, `is_sub_of`, `diff`, JSON) |
| `gmap.ListMap` / `ListKVMap` | insertion-ordered map | `gmap::ListMap` / `ListKVMap<K, V>` (vector of pairs; `Value` has no `Hash`, ADR-033) |
| `gmap.TreeMap` / `TreeKVMap` | red-black tree map | `gmap::TreeMap` = `gtree::RedBlackTree`; `TreeKVMap<K, V>`; constructors `new_tree_map*` |
| `gtree` boundary ops | tree navigation | `ceiling`, `floor`, `pop_left`, `pop_right`, `flip`, `set_comparator` (node-returning, ADR-030/137) |
| `gtree.AVLTree` / `AVLKVTree` | AVL tree | `gtree::AVLTree` = `AVLKVTree<Value, Value>`, generic `AVLKVTree<K, V>` |
| `gtree.RedBlackTree` / `RedBlackKVTree` | red-black tree | `gtree::RedBlackTree` = `RedBlackKVTree<Value, Value>`, generic `RedBlackKVTree<K, V>` |
| `gtree.BTree` / `BKVTree` | B-tree | `gtree::BTree` = `BKVTree<Value, Value>`, generic `BKVTree<K, V>` (`new(order, comparator)`) |
| `gtree.*Entry` / `*Node` | key-value pair | `BTreeEntry{key,value}`, `AVLTreeNode`, `RedBlackTreeNode` |
| `gtree.Left/Right/Floor/Ceiling/Flip/Height/String` | structural tree methods | same names (`left/right/floor/ceiling/flip/height/string`); `flip(Option<Comparator<Value>>)` |
| `gutil.Comparator*` | comparison functions | same names in `gf_util::gutil` taking `&Value` (`comparator_string`, `comparator_int`, ...); `comparator_t<T: Ord>`, `comparator_t_str<T: ToString>` |
| `gpage.Page` / `PageBar` | deprecated pager HTML | `gf_util::gpage::Page` / `page_bar()` with Go's five-character HTML escaping |
| `guid.S` | 32-byte unique string | `gf_util::guid::s(&[&[u8]])` (SDBM, MAC, sequence local to the crate, ADR-036) |
| `intlog.Print/Printf/Error/Errorf/PrintFunc/ErrorFunc` | internal debug log | `gf_internal::intlog::{print, printf, error, errorf, print_func, error_func}` (preformatted text) |
| `genv.All/Map/Get/Set/*` | environment access | `gf_os::genv::{all, map, get, set, ...}` (`get` returns `Option<Var>`, `map_from_env` returns `Result`) |
| `gbuild.Info/Get/Data` | injected build variables | `gf_os::gbuild::{info, get, data}` (`GF_BUILD_DATA` at compile time) |
| `gtime.Time` | time wrapper (`*Time`, may be nil) | `gf_os::gtime::Time` (`is_nil()`, `is_zero()`, `Time::default()` is nil) |
| `gtime.New/Now/NewFromStr/NewFromTimeStamp/NewFromTimestamp*` | constructors | `Time::{new, now, now_utc, new_from_str, new_from_time_stamp, new_from_timestamp*}` |
| `gtime.Format` / `Layout` | PHP-style / Go-layout format | `format` / `layout` (plus `format_to`, `format_new`, `layout_new`, `layout_to`) |
| `gtime.StrToTime` / `StrToTimeFormat` / `Parse` | lenient parsing | `str_to_time(str, Option<&str>)`, `str_to_time_format`, `parse` (all `Result`) |
| `gtime.StartOf*/EndOf*` | range boundaries | `start_of_*` / `end_of_*(nano_precision: bool)` |
| `gtime.Timestamp/TimestampMilli/Micro/Nano` | unix units | `timestamp*` on `Time` and module-level `gtime::timestamp*()` |
| `gtime.Scan/Value/MarshalJSON/UnmarshalJSON` | SQL/JSON bridge | `scan(&Value)`, `value()`, `marshal_json()`, `unmarshal_json(&str)` |
| `gtime.Location` / `FixedZone` | zone kind | `gtime::Location` with `Location::named`, `Location::fixed(name, offset)` |
| `gtime.SetTimeZone` | process time zone | `gtime::set_time_zone(&str) -> Result` (applied to future `now()`) |
| `gfile.Mkdir/Create/Open/OpenFile/OpenWithFlag*` | file creation and opening | `mkdir`/`create`/`open`/`open_append`/`open_with(&OpenOptions)` (no flag integers, ADR-040) |
| `gfile.Exists/IsDir/IsFile/Stat/IsEmpty` | inspection | `exists`/`is_dir`/`is_file`/`stat`/`is_empty` |
| `gfile.Move/Rename/Remove/RemoveFile/RemoveAll` | mutation | `rename`/`remove`/`remove_file`/`remove_all` |
| `gfile.Abs/RealPath/Pwd/Chdir/SelfPath/SelfName/SelfDir` | paths | `abs`/`real_path`/`pwd`/`chdir`/`self_path`/`self_name`/`self_dir` |
| `gfile.Basename/Name/Dir/Ext/ExtName/Join/Temp/Home` | path text | same names (`SEPARATOR`/`separator()` replace the `Separator` variable) |
| `gfile.Get/ PutContents* /GetBytes* /Truncate /ReadLines*` | contents | `get_contents`/`get_bytes`/`put_contents*`/`put_bytes*`/`truncate`/`read_lines(_bytes)` |
| `gfile.GetNextCharOffset* /GetBytesTilChar* /GetBytesByTwoOffsets*` | byte offsets | same names over `impl Read + Seek` (Go takes `io.ReaderAt`) |
| `gfile.Copy/CopyFile/CopyDir` + `CopyOption` | copying | `copy`/`copy_file`/`copy_dir` with `CopyOption { sync, preserve_mode, mode }` |
| `gfile.ScanDir*` / `ScanDirFile*` | directory scanning | `scan_dir(_func)`/`scan_dir_file(_func)` |
| `gfile.ReplaceFile*` / `ReplaceDir*` | text replacement | `replace_file(_func)`/`replace_dir(_func)` |
| `gfile.Search` | file search by name | `search(name, &priority_paths)` (no `MainPkgPath`, ADR-040) |
| `gfile.Glob` / `MatchGlob` / `filepath.Match` | globbing | `glob(pattern, only_names)`, `match_glob` (globstar), `path_match` (`path.Match`) |
| `gfile.Size/SizeFormat/ReadableSize/StrToSize/FormatSize` | sizes | same names (modes are `u32`, not `os.FileMode`) |
| `gfile.MTime/MTimestamp/MTimestampMilli` | modification times | same names returning `gtime::Time` / `i64` |
| `gfile.SortFiles` | directories first | `sort_files` |
| `gfile.GetContentsWithCache` / `GetBytesWithCache` | cached contents | `get_contents_with_cache`/`get_bytes_with_cache` with `CacheDuration::{Default, None, Forever, After}` (ADR-043) |
| `gfile.MainPkgPath` | Go `package main` path | not ported: no Go runtime frames (ADR-011, ADR-040) |
| `gspath.New` / `gspath.Get` | path manager creation | `SPath::new`, `gspath::get(root) -> Arc<SPath>` (one manager per root) |
| `gspath.Search` / `SearchWithCache` | file search under a root | `search(root, name, index_files)`, `SPath::search` (direct + cache lanes, ADR-091) |
| `SPath.Set` / `Add` / `Remove` / `Paths` / `AllPaths` / `Size` | root management | `set`/`add`/`remove`/`paths`/`all_paths` (sorted cache keys)/`size`; `new_with_cache` enables the watcher-refreshed index (ADR-091) |
| `gfsnotify.New` / `Add` / `AddOnce` / `Remove` / `RemoveCallback` | file watching | same names in `gf_os::gfsnotify` (`Add` returns `Arc<Callback>`) |
| `gfsnotify.Exit` | remove the calling callback | `gfsnotify::exit()` (thread-local request instead of a panic) |
| `gfsnotify.Event` / `Op` / `WatchOption` | event, operation bits, options | `Event { path, op }`, `Op` bit flags with `is_*`, `WatchOption { no_recursive }` |
| `fsnotify` (Go module) | OS watchers | `notify` crate 8.2.2 (ADR-042) |
| `gres.New` / `Instance` | resource managers | `gres::Resource::new`, `gres::instance(name)` (one `Arc` per name) |
| `gres.Add` / `Load` / `Pack*` / `Unpack*` | pack and unpack | same names in `gf_os::gres`, taking `&[u8]`; `PackToGoFile` not ported (ADR-044) |
| `gres.Get` / `GetWithIndex` / `GetContent` / `Contains` / `IsEmpty` | lookups | same names (`get` returns `Option<Arc<File>>`) |
| `gres.ScanDir*` / `Export` / `Dump` | traversal and export | `scan_dir(_file)`, `export`, `dump` with `ExportOption { remove_prefix }` |
| `gres.File` / `FileInfo` / `Option` | resource entry and pack options | `File` (`content`, `file_info`, `open`, `export`), `FileInfo`, `PackOption { prefix, keep_path }` |
| `gstr.TrimLeftStr` | strip leading occurrences | `gstr::trim_left_str` |
| `gcfg.Config` / `Instance` | configuration manager | `gcfg::Config` (`get`, `get_or`, `get_with_env`, `get_with_cmd`, `get_effective`, `data`), `gcfg::instance(name)` |
| `gcfg.Adapter` / `WatcherAdapter` | adapter and watch support | one `Adapter` trait with default no-op watcher methods (ADR-045) |
| `context.Context` in gcfg | operation details for watchers | `OperationContext { file_name, file_path, file_type, operation, key, value, content }` + `OperationType` (ADR-045) |
| `gcfg.AdapterFile` | file-backed adapter | `AdapterFile` (`new`, `set_path`, `add_path`, `get_file_path`, `set_content`, `get_content`, `remove_content`, `clear_content`, `set`, `clear`) |
| `gcfg.AdapterContent` | in-memory adapter | `AdapterContent::new(Some(content))`, `set_content` |
| `gcfg.WatcherRegistry` | logical watchers | `WatcherRegistry::{add, remove, names, is_watching, notify}` |
| `gcfg.Loader[T]` | struct binding and reload | `Loader<T: Default + DeserializeOwned>` (`load`, `get`, `watch`, `stop_watch`, `is_watching`, `on_change`, `set_converter`) |
| `gjson.LoadContentType` / `ContentType` | typed loading | `gjson::{load_content, load_content_type, content_type, is_valid_content_type}` |
| `glog.LEVEL_*` / `F_*` / `COLOR_*` | level, flag and color constants | same `UPPER_SNAKE` names in `glog` |
| `glog.New` / `Instance` / `DefaultLogger` | logger creation | `glog::Logger::new`, `glog::instance(name)`, `glog::default_logger()` |
| `glog.Config` | logger configuration | `glog::Config` with `set_*` accessors on `Logger` |
| `glog.Handler` / `HandlerInput` | content middleware | `Handler = Arc<dyn Fn(&mut HandlerInput)>`, `HandlerInput::next`, `handler_json()`, `handler_structure()` |
| `glog.Logger` chaining | `Cat`/`File`/`Level`/`Stdout`/`Header`/`Line`/`Async`/`Skip`/`To` | `with_cat`, `cat`, `with_file`, `with_level`, `with_stdout`, `with_header`, `with_line`, `with_async`, `skip`, `with_writer` (return a cloned `Logger`; ADR-146) |
| `gcmd.Init` / `GetOpt` / `GetArg` / `GetOptWithEnv` | package-level args | same names in `gcmd` (returning `Option<Var>`), `build_options`, `scan` |
| `gcmd.Parser` / `Parse` / `ParseArgs` | option parsing | `Parser::parse` / `Parser::parse_args` with `ParserOption { case_sensitive, strict }` |
| `gcmd.Command` / `Argument` | command tree | `Command::new` + `add_command(Arc<Command>)`, `Argument { name, short, default, brief, is_arg, orphan }`, `run_with_specific_args`, `help_text` |
| `gcmd.Function` / `FuncWithValue` | command callbacks | `Arc<dyn Fn(&Parser) -> Result<_, Error> + Send + Sync>` (no context) |
| `gcmd.NewFromObject` / `AddObject` | struct-tag commands | derive lane: `CommandObject` + `#[derive(CommandObject)]` + `new_from_object::<T>()` with `#[cmd(...)]` field tags; method-as-subcommand reflection not ported (ADR-108) |
| `gcache.New` / `NewWithAdapter` | cache creation | `gcache::Cache::new`, `Cache::new_lru(cap)`, `Cache::with_adapter` |
| `gcache.Cache` / `Adapter` / `Func` | cache API | `Cache::{set, get, get_or_set, update, update_expire, remove, size, keys, values, clear, close, must_get, must_keys, must_values, must_size, must_data, must_key_strings, must_get_or_set_func_lock}`, `Adapter` trait, `Func = Arc<dyn Fn() -> Result<Value, Error>>` (ADR-150) |
| `time.Duration` in gcache | never / delete / expire | `Ttl::{Never, Remove, After(Duration)}`, `From<Duration>` |
| `any` cache keys | typed hashable keys | `Key` (canonical `type:text` + original `Value`), `impl Into<Key>` for `&str`/`String`/`i64`/`u64`/`Value` |
| `gmlock.Lock` / `Unlock` | keyed write lock | `gmlock::lock(key)` returns a `WriteGuard`; dropping it unlocks (ADR-049) |
| `gmlock.RLock` / `TryLock*` / `*Func` | keyed read locks and closures | `rlock`, `try_lock`, `try_rlock`, `lock_func`, `rlock_func`, `try_lock_func`, `try_rlock_func`, `remove` |
| `grpool.New` / `Add` | worker pool | `grpool::Pool::new(limit)`, `add(Arc<dyn Fn()>)`, `add_with_recover`, `pause`/`resume`/`close`, `set_cap`/`cap`, `size`/`jobs` |
| `grpool.Func` / `RecoverFunc` / `LimitChangerFunc` | pool callbacks | `Func = Arc<dyn Fn() + Send + Sync>`, `RecoverFunc = Arc<dyn Fn(&Error)>`, `LimitChangerFunc = Arc<dyn Fn(&AtomicI64) -> bool>` (no context) |
| `gfpool.Open` / `Get` | pooled file handles | `gfpool::open(path, FileOptions, Ttl)`, `get` (`FileOptions::{read_only, append_create, write_truncate}`) |
| `gfpool.File` / `Pool` | handle and pool | `File::{close, close_now, path, is_open}` (+ `Read`/`Write`/`Seek`), `Pool::{new, file, close}` |
| `gtimer.New` / `Add*` | timing jobs | `gtimer::Timer::new(options)`, `add`, `add_entry`, `add_singleton`, `add_once`, `add_times`, `delay_add*`, `start`/`stop`/`close` |
| `gtimer.Entry` / `JobFunc` / `Exit` | job handle and callbacks | `Entry::{status, run, start, stop, close, reset, is_singleton, set_singleton, set_times}`, `JobFunc = Arc<dyn Fn() + Send + Sync>`, `gtimer::exit()` (thread-local) |
| `os/gctx`, `os/gstructs`, `util/gmeta` | context/reflection helpers | not ported: no context, no runtime reflection (ADR-052) |
| `gcron.New` / `Add*` | cron manager | `gcron::Cron::new`, `add`, `add_singleton`, `add_once`, `add_times`, `add_entry`, `delay_add*`, `start`/`stop`/`close` |
| `gcron.Entry` / `JobFunc` | cron entry and job | `Entry::{name, pattern, status, start, stop, close, set_times, set_singleton}`, `JobFunc = Arc<dyn Fn() + Send + Sync>` |
| `gcron.schedule.Next` | next activation time | `CronSchedule::next(DateTime<Local>)` over `chrono` |
| `gcron.StopGracefullyNonBlocking` | graceful stop handle | `Cron::stop_gracefully_non_blocking` returns a `StopWaiter` with `wait()` |
| `gsession.Manager` / `Session` | session manager | `Manager::{new, new_session, set_storage, storage, ttl}`, `Session::{set, get, remove, remove_all, data, size, contains, id, close, regenerate_id, must_*}` |
| `gsession.Storage` / `StorageMemory` / `StorageFile` | session storages | `Storage` trait with `error_disabled()` defaults, `StorageMemory::new`, `StorageFile::new(path, ttl)` |
| `gsession.NewSessionId` | session id | `gsession::new_session_id()` (32-char `guid::s`) |
| `context.Context` in gsession | request context | not ported: storage methods take data arguments only (ADR-055) |
| `gmetric.GetGlobalProvider` / `Meter` | metric creation | `gmetric::get_global_provider().meter(option)` returns `LocalMeter`; `Meter::{counter, up_down_counter, histogram, observable_*, must_*}` |
| `gmetric.MetricOption` / `Option` | metric and measurement options | `MetricOption { help, unit, attributes, buckets, callback }`, `ObservationOption { attributes }` (Go `Option` renamed) |
| `gmetric.Provider` / `MeterPerformer` | metric backend | traits; `set_global_provider`, `is_enabled`, no-op performers by default |
| `gmetric.Attributes` / `Attribute` | metric metadata | `Attribute { key, value }`, `Attributes = Vec<Attribute>`, `new_attribute`, `common_attributes` |
| `gmetric.SetGlobalAttributes` / `GetGlobalAttributes` | global attributes | same names with `SetGlobalAttributesOption`/`GetGlobalAttributesOption` matching rules |
| `gproc.ShellExec` / `ShellRun` / `Shell` | shell execution | `gproc::{shell_exec, shell_run, shell}` (no context) |
| `gproc.Pid` / `PPid` / `IsChild` / `SetPPid` / `Uptime` | process identity | `pid`, `ppid`, `is_child`, `set_ppid`, `start_time`, `uptime`; `ppid_os` is Linux-only |
| `gproc.SearchBinary*` | binary lookup | `search_binary`, `search_binary_path` (`PATH` split, `.exe` on Windows) |
| `gproc` comm/signal/manager | goroutine processes | not ported: no goroutine ids or `std` signals (ADR-057) |
| `gview.New` / `Parse*` | template engine | `View::{new, with_path, parse, parse_content, parse_default, parse_default_content, parse_with_options}` |
| `gview.Config` | template configuration | `Config { paths, data, default_file, delimiters, auto_encode }`, `set_config`, `set_config_with_map` |
| `gview.FuncMap` / `BindFunc` | template functions | `FuncValue = Arc<dyn Fn(&[Value]) -> Result<Value, Error>>`, `bind_func`, `bind_func_map` |
| `gview` engine | Go text/template | own subset parser/evaluator (`if`/`range`/`with`/pipelines/include), not Go-compatible in full, ADR-058 |
| `gview` assignment | variable declarations | `Node::Assign { name, expr }`, `parse_assignment`; `{{$x := ...}}`/`{{$x = ...}}` (ADR-119) |
| `gview` std builtins | Go template functions | `printf` (subset), `len`, `index`, `slice`, `and`, `or`, `not` (ADR-107) |
| `gview` definitions | define/template/block | `Node::Template`, `split_definitions`, `find_matching_end` (ADR-106) |
| `gview` break/continue | range control flow | `Node::Break`/`Node::Continue`, internal `Flow` returned by `render_nodes` (ADR-145) |
| `gview` i18n | result translation | `View::{set_i18n, i18n}`, `I18nLanguage` param override (ADR-088) |
| `gtrace.Carrier` | text-map propagation carrier | `gtrace::Carrier::{new, get, set, keys, must_marshal, unmarshal_json}` |
| `gtrace.TraceContext` / `Baggage` | explicit trace and baggage holders | `TraceContext::{new, trace_id, span_id, has_trace_id}`, `gtrace::{with_trace_id, with_uuid, get_trace_id, get_span_id}`, `Baggage::{set_value, set_map, get_map, get_var}` |
| `gtrace.SafeContent*` | trace content cutting | `safe_content`, `safe_content_for_http` (gzip aware), `max_content_log_size` |
| `gtrace` spans/provider/propagator | OTel integration | not ported: needs `context.Context` and OpenTelemetry (ADR-059) |
| `gudp.ClientConn` / `ServerConn` | UDP connections | `ClientConn::{new, send, recv, send_recv, remote_addr, local_addr, set_deadline*}`, `ServerConn::{recv, send, is_closed, set_deadline*}` |
| `gudp.Server` / `GetServer` | UDP server and registry | `Server::{new, set_address, set_handler, run, close, listened_address, listened_port, local_addr}`, `get_server` |
| `gudp.Send` / `SendRecv` / `GetFreePorts` | package helpers | `send`, `send_recv`, `get_free_port`, `get_free_ports` |
| `gudp.Retry` | retry options | `Retry { count, interval }` (by value; zero interval = 100 ms), ADR-060 |
| `gtcp.Conn` | TCP connection | `Conn::{new, from_stream, send, recv, recv_line, recv_till, recv_with_timeout, send_with_timeout, send_recv*, set_deadline*, set_buffer_wait_recv, close}` |
| `gtcp.PkgOption` | package protocol options | `PkgOption { header_size, max_data_size, retry }`, `send_pkg*`, `recv_pkg*`, `get_pkg_option` |
| `gtcp.Server` / `GetServer` | TCP server and registry | `Server::{new, set_address, address, set_handler, run, close, listened_address, listened_port, local_addr}`, `get_server` |
| `gtcp` TLS | TLS conns | not ported: needs a TLS stack (ADR-061) |
| `gtcp.PoolConn` | pooled TCP connection | `gtcp::pool::PoolConn::{new, send, recv, recv_line, recv_till, *_with_timeout, *_pkg*, close, conn, status}`, `DEFAULT_POOL_EXPIRE`, `CONN_STATUS_*` (ADR-064) |
| `goai.OpenApiV3` | OpenAPI document | `OpenApiV3::{new, add_schema, add_path, add_operation, find_operation, add_server, add_tag, path_mut, to_json, to_pretty_json}`; model types (`Info`, `Path(s)`, `Operation`, `Parameter`, `RequestBody`, `Response(s)`, `Schema(s)`, `Components`, security, servers, tags) |
| `goai` refs | reference-or-value | generic `RefOr<T> { reference, value: Box<T> }` serializing as `{"$ref": ...}` or the inline value |
| `goai` struct reflection | `Add`/schema from structs | not ported: manual `add_operation` plus `schema_from_value` instead (ADR-065/085) |
| `ghttp.Header` | HTTP header map | `Header::{new, set, add, get, values, del, contains, len, iter}`, `canonical_key` |
| `ghttp.Cookie` / `Cookies` | cookies | `Cookie::{new, to_set_cookie}`, `parse_set_cookie`, `Cookies::{parse, get, set, remove, iter}` |
| `ghttp.Query` | query/form values | `Query::{parse, get, values, add, encode}`, `url_decode`, `url_encode` |
| `ghttp.Request` / `Response` | HTTP/1.1 messages | `Request::parse`, `Request::{query_value, form}`, `Response::{new, set_body, set_text, set_json, write_to, to_http_string}` |
| `ghttp.Server` / `Router` | HTTP server + router | `Server::{new, with_address, bind_handler, bind_middleware, group, serve, run, close, listened_address, listened_port}`, `Router::{bind, bind_with_middlewares, lookup}` |
| `ghttp` server lifecycle | keep-alive, status, shutdown | `Server::{status, bind_status_handler, bind_status_handler_by_map, shutdown}`, `SERVER_STATUS_*` (ADR-073) |
| `ghttp` tracing | W3C traceparent propagation | `tracing::{ServerSpan, parse_traceparent, traceparent_header, middleware_tracing}`, `Context::span` (ADR-105) |
| `ghttp` middleware | JSON body / response envelope | `middleware::{middleware_json_body, middleware_handler_response, DefaultHandlerResponse, NEED_JSON_BODY}`, deferred `middleware::finish` (ADR-104) |
| `ghttp` gzip | response compression | `gzip::{middleware_gzip, GZIP_MIN_LENGTH}`, `Context::gzip` flag + `gzip::finish` after the chain (ADR-103) |
| `ghttp` CORS | cross-origin support | `cors::{CorOptions, default_options, allowed_origin, middleware_cors}`, `Context::{cors, cors_default}`, `Context::exit_all` (ADR-102) |
| `ghttp` config map | server configuration | `Server::{set_config_with_map, set_openapi_paths}` (subset: address/serverRoot/indexFiles/indexFolder/staticPaths/openapiPath/swaggerPath/clientMaxBodySize, ADR-090/126) |
| `ghttp` openapi | OpenAPI/Swagger serving | `openapi::{OpenApiConfig, swagger_ui_html}`, `OpenApiConfig::{new, with_openapi_path, with_swagger_path, document_json, ui_html, serve}`, `Server::set_openapi` (ADR-080) |
| `ghttp` cookies / session | response cookies and request sessions | `Cookie::{path, domain, max_age, secure, http_only, same_site}`, `Response::{add_cookie, set_cookie, cookies}`, `Server::{set_session_manager, set_session_cookie_name}`, `Context::session_mut` (ADR-074) |
| `ghttp` hooks | serve/output hooks | `HookName::{BeforeServe, AfterServe, BeforeOutput, AfterOutput}`, `Server::{bind_hook_handler, bind_hook_handler_for}`, `router::pattern_matches` (global + pattern, ADR-127/129) |
| `ghttp` body limit | request size limit | `Server::set_client_max_body_size`, 413 `Request Entity Too Large` (ADR-125) |
| `ghttp` header limit | max header bytes | `Request::parse_with_limit`, `Server::{set_max_header_bytes, max_header_bytes}`, 431 on overflow (ADR-170) |
| `ghttp` chunked | chunked bodies | `Request::parse` reads chunked requests (`read_chunked_body`, hex sizes/trailers); `Response::write_to` writes chunked responses (ADR-155/156) |
| `ghttp` multipart | form-data parsing | `Request::multipart_form`, `MultipartForm { boundary, parts }`, `MultipartPart { name, file_name, content_type, data }`, `value`/`file` lookups (ADR-154) |
| `ghttp` response writing | buffered response helpers | `Response::{write, write_json, write_status, buffer, buffer_length, clear_buffer, set_status, status_code, header, set_header}` (ADR-148) |
| `ghttp` cookie config | session cookie attributes | `CookieConfig { domain, path, max_age, secure, http_only, same_site }`, `Server::{set_cookie_config, cookie_config, set_cookie_*}` (ADR-144) |
| `ghttp` HTTP/2 decision | not ported | h2/ALPN/fd passing documented as non-ports, reverse proxy recommended (ADR-190) |
| `ghttp` reuse port | SO_REUSEPORT restart half | `Server::set_reuse_port`/`reuse_port`, `bind_reuse_port` (Unix socket2, ADR-188) |
| `ghttp` admin | admin endpoints | `Server::enable_admin(pattern)` → index/restart(501)/shutdown, `DEFAULT_ADMIN_PATH` (ADR-185) |
| `ghttp` domains | per-host routes | `path@domain` patterns, `Route.domain`, host-specific priority (ADR-180) |
| `ghttp` context getters | unified request values | `Context::{get, get_value, get_string, get_map}` (params → form → query), `Query::entries` (ADR-158) |
| `ghttp` route listing | router map | `Router::route_list`, `Server::routes` (ADR-143) |
| `ghttp` redirects | 302/301 redirects | `Context::{redirect_to, redirect_back}`, `Location` header + `exit_all` (ADR-124) |
| `ghttp` request helpers | client IP / URL / AJAX | `Request::{client_ip, remote_ip, schema, url, is_ajax_request, referer}`, `Request.remote_addr` (ADR-118) |
| `ghttp.Context` / `Group` | request context and groups | `Context::{new, proceed, param, redirect_to, redirect_back}`, `Group::{new, group, middleware, bind_handler, bind, get, post, put, delete, patch, options, head, all, bind_methods}` (ADR-112), `not_found`, `internal_error`, `write_json`, `write_text` |
| `ghttp` static files | static file serving | `Server::{set_server_root, add_search_path, add_static_path, set_index_files, set_index_folder, set_file_server_enabled}`, `file_server::{StaticConfig, serve, apply, apply_conditional, http_date, parse_http_date, path_escape, html_escape}`, `ServeResult::File { modified }`, ETag/Last-Modified/304 (ADR-068/113) |
| `gclient.Client` | HTTP client | `Client::{new, set_prefix, set_header, add_header, set_cookie, set_timeout, set_retries, get, get_content, post, post_form, post_json, put, delete, head, patch, request}` |
| `gclient.Content` | response content | `Content::from_response` (JSON or text) |
| `gclient` methods | extra HTTP verbs | `Client::{options, trace, connect}` (no CONNECT tunnel, ADR-176) |
| `gclient` chunked | response body decoding | `parse_response` decodes chunked framing and truncates to `Content-Length` (ADR-157) |
| `gclient` redirects | 3xx handling | `Client::{max_redirects, set_max_redirects}`, `DEFAULT_MAX_REDIRECTS`, `is_redirect`, `resolve_url` (ADR-082) |
| `gclient` HTTPS | TLS client | `Client::{add_root_certificate_pem, add_root_certificate_der}`, rustls with Mozilla roots (ADR-083) |
| `ghttp` HTTPS server | TLS server | `Server::{set_tls_pem, is_tls}`, rustls server config (ADR-084) |
| `gtcp` TLS client | TLS TCP client | `gtcp::tls::{TlsConn, TlsClientConfig}`, `TlsConn::{new, send, recv, set_deadline, close}` (ADR-086) |
| `gtcp` TLS server | TLS TCP server | `gtcp::tls::TlsServer::{new, set_tls_pem, set_handler, run, close, listened_port}`, handler receives `TlsConn` (ADR-087) |
| `gredis` pipeline/tx | batch commands | `Client::{pipeline, watch, unwatch, transaction, watch_transaction}` (ADR-097) |
| `gredis` scripts | Lua scripting | `Client::{eval, eval_sha, script_load, script_exists, script_flush}` (ADR-099) |
| `gredis` pub/sub | subscriptions | `Client::{subscribe, psubscribe}`, `Subscription::{next_message, unsubscribe, close}`, `SubscribeMessage { pattern, channel, payload }` (ADR-098) |
| `gredis` ACL auth | username/password AUTH | `Config::{user, with_user}`, `AUTH <user> <pass>` (ADR-128) |
| `gredis.Client` | Redis client | `gredis::{Client, Config, Reply}`, `Client::{new, with_address, command, ping, get, set, set_ex, del, exists, expire, ttl, incr, decr, h*, l*, s*, z*, publish, close}` |
| `gredis` pool | connection pooling | `RedisPool { new, get, close, idle_count, total, created_count }`, `PooledClient` (derefs to `Client`), `Config::{with_pool, with_pool_timeouts, with_wait_timeout}` (ADR-120) |
| `gredis` replica round-robin | read distribution | `ClusterClient` replica cursor over `SlotRange.replicas` when `slave_only` (ADR-159) |
| `gredis` replicas | read-only cluster routing | `Config::{slave_only, with_slave_only}`, `SlotRange.replicas` (ADR-139) |
| `gredis` cluster | slot routing | `ClusterClient { new, command, ping, slots, close }`, `SlotRange`, `Config::with_cluster`, `key_slot`, `parse_slot_ranges` (ADR-116) |
| `gredis` sentinel | master discovery | `Config::{master_name, sentinel_user, sentinel_pass, with_master_name, with_sentinel_auth}`, `resolve_sentinel_master` (ADR-114) |
| `gredis` RESP | protocol codec | `encode_command`, `decode_reply`, `Reply::{string, bulk, int, strings, pairs, is_error}`; RESP3 variants `Map/Set/Double/Boolean/BigNumber/Push/Null` and `Client::{hello, resp3}` (ADR-070/111) |
| `gredis` cluster/pubsub | advanced features | Sentinel (ADR-114) and slot-routing cluster (ADR-116) are ported; cluster replicas/read splitting and pooling remain unported (ADR-070/097/098/099/111) |
| `gdb.Record` / `Result` | DB row types | `Record::{new, from_map, insert, get, string, int, uint, map, json, xml}`, `Result::{is_empty, len, size, chunk, json, xml, list, array, map_key_*, record_key_*}`, `Map`/`List`/`Array` aliases |
| `gdb.SqlResult` | SQL execution result | `SqlResult { affected, last_insert_id }`, `rows_affected`, `last_insert_id`, `insert_id`, `must_get_*` |
| `gdb.Struct(s)` / `ScanList` | struct binding | not ported: needs runtime reflection (ADR-071) |
| `gdb.Builder` | SQL clause builder | `gdb_builder::{Builder, Dialect, WhereHolder, WhereKind, WhereOperator}`, `Builder::{new, where_*, or_where_*, where_in, where_like, where_between, where_null, where_raw, order_*, group, having, limit, page, select_sql, insert_sql, update_sql, delete_sql}` (ADR-072) |
| `gdb.Db` | SQLite execution | `gdb_exec::Db::{open, open_memory, execute, query, insert, update, delete, select, find, scalar, dialect}` (ADR-076) |
| `gdb` MySQL/PostgreSQL/pooling | other drivers | not ported: SQLite only (ADR-076) |
| `gdb` transactions | tx helpers | `Db::{begin, commit, rollback, is_transaction, transaction}`, `Model::transaction` (ADR-079) |
| `gdb.DbPool` | SQLite connection pool | `DbPool::{open, open_with_size, get, try_get, size, idle, path}`, `PooledDb` guard (ADR-096) |
| `gdb` sharding | table sharding | `ShardingConfig { table: Option<ShardHandler> }`, `ShardHandler`, `Model::sharding(&value, config)` (ADR-130) |
| `gdb` chunk | paged iteration | `Model::chunk(size, handler)` (ADR-123) |
| `gdb` MySQL | MySQL execution backend | `MysqlBackend`, `Db::open_mysql`, `DbPool::open_mysql*`, `mysql_options/params/value` (ADR-189) |
| `gdb` backends | SQLite/PostgreSQL execution | `gdb_exec::Backend` (crate-private), `SqliteBackend`, `PostgresBackend`, `Db::open_postgres`, `DbPool::open_postgres*` (ADR-187) |
| `gdb` fields-prefix | alias-qualified projection | `Model::fields_prefix(prefix, fields)` (ADR-178) |
| `gdb` upsert | INSERT ... ON CONFLICT | `Builder::insert_on_conflict_sql`, `Model::insert_on_conflict(data, key_fields)` (MySQL/SQLite/PG, ADR-171) |
| `gdb` replace | REPLACE statement | `Builder::replace_sql`, `Model::replace` (MySQL/SQLite; PostgreSQL errors toward `save`, ADR-169) |
| `gdb` count-column/quote | column count + quoting | `Model::count_column(field)`, `Builder::quote_word`/`Model::quote_word` (ADR-168) |
| `gdb` raw select | hand-written SELECT | `Builder::raw(sql, args)`, `Model::raw` (ADR-167) |
| `gdb` alias/batch | table alias + batch insert | `Builder::as_alias`, `Model::{as_alias, batch(data, batch_size)}` (ADR-166) |
| `gdb` data/array | row/column extraction | `Model::{data, array(field)}` (ADR-165) |
| `gdb` increment | atomic field math | `Builder::{increment}`, `Model::{increment, decrement}` (SQL `field = field + n`, ADR-164) |
| `gdb` model flags | offset/omit/field checks | `Model::{offset, omit_empty, omit_nil, has_field, insert_and_get_id}` (ADR-163) |
| `gdb` fields-ex | exclude columns | `Model::fields_ex(&[&str])` (table metadata, case-insensitive, ADR-162) |
| `gdb` prefix/suffix | LIKE helpers | `Builder::{where_prefix, where_suffix, or_where_prefix, or_where_suffix}`, `Model::*` (ADR-161) |
| `gdb` distinct | `SELECT DISTINCT` | `Builder::distinct`, `Model::distinct` (ADR-160) |
| `gdb` save | upsert by explicit key fields | `Model::save(data, key_fields)` (ADR-152) |
| `gdb` insert-ignore | duplicate-key-tolerant insert | `Builder::insert_ignore_sql`, `Model::insert_ignore` (dialect-specific, ADR-149) |
| `gdb` joins | multi-table queries | `JoinKind`, `JoinClause`, `Builder::{join, left_join, right_join, inner_join}`, `Model::{left_join, right_join, inner_join}` with raw `ON` (ADR-147) |
| `gdb` NOT/OR conditions | where helpers | `where_not_between`, `where_not_like`, `where_raw`, `or_where_{op,gt,ge,lt,le,ne,like,not_like,in,not_in,between,not_between,null,not_null,raw}` (ADR-142) |
| `gdb` aggregates | SQL aggregates | `Model::{aggregate, sum, avg, max, min}` (ADR-121) |
| `gdb` model cache | TTL result cache | `Model::cache(Duration)`, `MODEL_CACHE`, `CacheEntry` (ADR-117) |
| `gdb` time fields | automatic timestamps / soft delete | `TimeOption { created_at, updated_at, deleted_at, maintain_disabled, soft_delete_disabled }`, `Model::{with_time_option, unscoped}` (ADR-110) |
| `gdb` table fields | column introspection | `FieldInfo { name, type_name, nullable, primary, default }`, `Db::table_fields`, `Model::table_fields` (ADR-109) |
| `gdb` struct writes | derive-based writes | `ToColumnValue`, `ToRecord`, `Model::{insert_record, update_record}`, `#[derive(ToRecord)]` with `#[gdb(column/skip)]` (ADR-122) |
| `gdb` struct scanning | derive-based row mapping | `FieldFromValue`, `FromRecord`, `Model::{scan, scan_one}`, `#[derive(FromRecord)]` with `#[gdb(column/skip)]` (ADR-100) |
| `gdb.Model` | chainable ORM | `gdb_model::Model::{new, cache, with_time_option, unscoped, table_fields, fields, where_*, order_*, group, having, limit, page, all, one, find, count, value, exists, sum, avg, max, min, aggregate, chunk, sharding, insert, update, delete, transaction}` (ADR-077/109/110/117/121), `record_data`, `map_data` (ADR-077) |
| `gsvc.Service` / `LocalService` | service definition | `Service` trait (`get_name/version/key/value/prefix/metadata/endpoints`), `LocalService::{new_service_with_name, new_service_with_kv}` |
| `gsvc.Endpoint` / `Metadata` | service address and metadata | `Endpoint::{new, must_new, host, port}`, `Endpoints::{new, as_slice, len, is_empty}` (Display), `Metadata::{set, sets, get, is_empty, values}` |
| `gsvc.Registry` | registry/discovery interfaces | `Registrar`/`Discovery`/`Watcher`/`Registry` traits, `set_registry`, `get_registry`, `register`, `deregister`, `SearchInput` |
| `gsel.Selector` | load balancer | `Selector::{pick, update}`, `Node`, `Picked`, `DoneInfo`, `DoneFunc`; `SelectorRoundRobin`/`SelectorRandom`/`SelectorWeight`/`SelectorLeastConnection` |
| `gsel.Builder` | selector builders | `Builder` trait, `new_builder_*`, `set_builder`, `get_builder` |
| `ghttp/internal/response.Writer` | response writer wrapper | `response::ResponseWriter` trait, `Writer::{new, write_header, write, flush, bytes_written, is_header_wrote, is_hijacked, mark_hijacked}` |
| `ghttp/internal/response.BufferWriter` | buffered response writer | `BufferWriter::{new, write, write_string, buffer, buffer_string, buffer_length, set_buffer, clear_buffer, write_header, flush}`, `status_text` |
| `io.Writer` as logger output | custom sink | `glog::Writer = Arc<dyn Fn(&str) + Send + Sync>` |
| `VERSION` | framework version | `gf_core::VERSION`, re-exported as `gf::VERSION` |
| `[]byte` in JSON | BASE64 string | `marshal_value` encodes it that way (ADR-014) |
| `empty.IsEmpty` / `empty.IsNil` | empty/nil checks | `gf_internal::empty::{is_empty, is_nil}` over `&Value` |
| `utils.IsInt` / `IsUint` / `IsFloat` / `IsSlice` / `IsMap` | kind predicates | same names over `&Value` |
| `utils.IsStruct` / `CanCallIsNil` | Go kind inspection | not ported (ADR-011) |
| `utils.IsNumeric` / `IsASCII` / `IsLetter*` | byte/string checks | same names, `&str` / `u8` |
| `utils.UcFirst` | ASCII first-letter upper case | `uc_first` |
| `utils.Trim` / `SplitAndTrim` | cutset trimming and splitting | `trim(s, Option<&str>)` / `split_and_trim` |
| `utils.RemoveSymbols` / `EqualFoldWithoutChars` | symbol stripping and folded compare | `remove_symbols` / `equal_fold_without_chars` |
| `utils.FormatCmdKey` / `FormatEnvKey` | key normalization | `format_cmd_key` / `format_env_key` |
| `utils.StripSlashes` | backslash unquoting | `strip_slashes` |
| `utils.ListToMapByKey` | group maps by a key | `list_to_map_by_key(&[Value], &str)` |
| `utils.MapPossibleItemByKey` | the internal lookup behind `gutil.MapPossibleItemByKey` | see the `gutil` row above |
| `utils.ReadCloser` | repeatable body reader | `gf_internal::utils::ReadCloser` (`read` returns `(n, eof)`) |
| `utils.IsDebugEnabled` / `SetDebugEnabled` | debug flag | `is_debug_enabled` / `set_debug_enabled` |
| `fileinfo.Info` | virtual file description | `gf_internal::fileinfo::Info` |
| `deepcopy.Copy` | deep copy | `gf_internal::deepcopy::copy` (`Value::clone`) |
| `safe ...bool` (containers) | synchronized switch | ignored; containers are always synchronized (ADR-007) |
| `context.Context` | request/operation context | not ported: explicit parameters/typed structs instead (ADR-034, ADR-052) |
| `gvar.Var` | dynamic value wrapper | `gf_container::gvar::Var` |
| `GMap` / `GArray` / `GSet` / `GList` / `GQueue` / `GRing` / `GTree` / `GType` / `GPool` | containers | same type names in `gf_container` |
| `gpool.TPool[T]` / `gpool.Pool` | object pool | `gf_container::gpool::{TPool<T>, Pool}` (`Pool` = `TPool<Value>`) |
| `gpool.NewFunc` / `ExpireFunc` | pool callbacks | `TPoolNewFunc<T>` / `TPoolExpireFunc<T>` (boxed, optional) |
| `ttl time.Duration` (gpool) | signed idle lifetime | `gpool::Ttl::{Never, Immediate, After(Duration)}` (new name, ADR-038) |
| `gmutex.Mutex` / `RWMutex` / `New` | lock helpers | `gf_os::gmutex::{Mutex, RWMutex, new}` (`lock`/`rlock` return guards) |
| `gtype.Float32/64.String` | Go `'g'` float text | `gf_core::float::format_g32` / `format_g` |
| `gconv.String(float)` | Go `'f'` float text | `gf_core::float::format_f` |
| `gtree.SetComparator` / `PopLeft` / `PopRight` | reorder, remove extremes | `set_comparator`, `pop_left`, `pop_right` on the ordered trees |
| `gtree.UnmarshalJSON` / `UnmarshalValue` | tree JSON input | `unmarshal_json` / `unmarshal_value` on the dynamic trees |
| `gconv` | type conversion | `gf_util::gconv` |
| `gstr` | string helpers | `gf_text::gstr` |
| `gtag` | struct tag parsing | `gf_util::gtag` |
| `gmeta` | struct metadata | `gf_util::gmeta` |
| `gcfg` | configuration | `gf_os::gcfg` |
| `glog` | logging | `gf_os::glog` |
| `ghttp.Server` | HTTP server | `gf_net::ghttp::Server` (own HTTP/1.1 server + router, ADR-067) |
| `gdb` | ORM/database abstraction | `gf_database::gdb` |
| `i18n` accept-language | HTTP language negotiation | `gi18n::{parse_accept_language, negotiate_language}`, `ghttp::language::middleware_accept_language`, `Context::language` (ADR-138) |
| `i18n` auto reload | file-change refresh | `Options::with_auto_reload`, `Manager::reload`, `source_stamp` (ADR-115) |
| `i18n` packed resource | gres-backed translations | `gi18n::{Resource, ResourceFile}`, `Options::with_resource`; `gres::Resource` implements it (ADR-101) |
| `i18n.Manager` | translations | `gi18n::{Manager, Options}`, `Manager::{set_path, set_language, set_delimiters, translate, translate_with_language, translate_format, get_content, get_content_with_language}`, `instance`/`set_instance` (ADR-078) |
| `i18n` context/gres lanes | language from context, packed resources | not ported: `*_with_language` methods instead (ADR-078) |
| `test/gtest` | assertions | `gf_test::gtest::{assert_eq, assert_eq_str, assert_ne, assert_gt, assert_ge, assert_lt, assert_le, assert_in, assert_ni, assert_nil, assert_not_nil, error, fatal}` (ADR-089) |
| `gvalid` struct lane | derive-based validation | `Validatable`, `FieldValue`, `Validator::check_struct`, `#[derive(Validate)]` with `#[gf(rule = "...")]` (ADR-092) |
| `frame/gins` | instance container | `gins::{server, set_server, log, config, i18n, set_i18n, view, set_view}`, `FRAME_CORE_COMPONENT_NAME_*` (ADR-081) |
| `frame/g` | facade accessors (`g.Server()`, `g.DB()`) | `gf::frame::g` with `server`/`tcp_server`/`udp_server`/`client`/`log`/`cfg`/`i18n`/`view`/`validator`/`set_debug` (delegating to `frame::gins`) and the `Var`/`Map`/`List`/`Slice`/`Array` aliases (ADR-075); `DB`/`Redis`/`I18n` not provided |
