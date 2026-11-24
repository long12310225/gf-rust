package main

import (
	"fmt"
	"runtime"
	"time"

	"github.com/gogf/gf/v2/container/garray"
	"github.com/gogf/gf/v2/container/glist"
	"github.com/gogf/gf/v2/container/gmap"
	"github.com/gogf/gf/v2/container/gqueue"
	"github.com/gogf/gf/v2/container/gring"
	"github.com/gogf/gf/v2/container/gset"
	"github.com/gogf/gf/v2/container/gtree"
	"github.com/gogf/gf/v2/container/gtype"
	"github.com/gogf/gf/v2/container/gvar"
	"github.com/gogf/gf/v2/crypto/gaes"
	"github.com/gogf/gf/v2/crypto/gcrc32"
	"github.com/gogf/gf/v2/crypto/gmd5"
	"github.com/gogf/gf/v2/crypto/gsha256"
	"github.com/gogf/gf/v2/debug/gdebug"
	"github.com/gogf/gf/v2/encoding/gbase64"
	"github.com/gogf/gf/v2/encoding/gbinary"
	"github.com/gogf/gf/v2/encoding/gcharset"
	"github.com/gogf/gf/v2/encoding/gcompress"
	"github.com/gogf/gf/v2/encoding/ghash"
	"github.com/gogf/gf/v2/encoding/ghtml"
	"github.com/gogf/gf/v2/encoding/gjson"
	"github.com/gogf/gf/v2/encoding/gproperties"
	"github.com/gogf/gf/v2/encoding/gtoml"
	"github.com/gogf/gf/v2/encoding/gurl"
	"github.com/gogf/gf/v2/encoding/gxml"
	"github.com/gogf/gf/v2/encoding/gyaml"
	"github.com/gogf/gf/v2/errors/gerror"
	"github.com/gogf/gf/v2/internal/command"
	"github.com/gogf/gf/v2/internal/empty"
	"github.com/gogf/gf/v2/internal/json"
	"github.com/gogf/gf/v2/internal/utils"
	"github.com/gogf/gf/v2/net/gipv4"
	"github.com/gogf/gf/v2/os/genv"
	"github.com/gogf/gf/v2/os/gtime"
	"github.com/gogf/gf/v2/text/gregex"
	"github.com/gogf/gf/v2/text/gstr"
	"github.com/gogf/gf/v2/util/gconv"
	"github.com/gogf/gf/v2/util/gpage"
	"github.com/gogf/gf/v2/util/grand"
	"github.com/gogf/gf/v2/util/guid"
	"github.com/gogf/gf/v2/util/gutil"
)

var sink uint64

func report(name string, iters int, run func()) {
	for i := 0; i < iters/10; i++ {
		run()
	}
	best := float64(1 << 62)
	for round := 0; round < 5; round++ {
		start := time.Now()
		for i := 0; i < iters; i++ {
			run()
		}
		ns := float64(time.Since(start).Nanoseconds()) / float64(iters)
		if ns < best {
			best = ns
		}
	}
	fmt.Printf("%s\t%d\t%.1f\n", name, iters, best)
}

func main() {
	buffer := make([]byte, 1024)
	for i := range buffer {
		buffer[i] = 0x5a
	}

	report("ghash_16x1KiB", 50000, func() {
		var acc uint64
		acc ^= uint64(ghash.AP(buffer))
		acc ^= ghash.AP64(buffer)
		acc ^= uint64(ghash.BKDR(buffer))
		acc ^= ghash.BKDR64(buffer)
		acc ^= uint64(ghash.DJB(buffer))
		acc ^= ghash.DJB64(buffer)
		acc ^= uint64(ghash.ELF(buffer))
		acc ^= ghash.ELF64(buffer)
		acc ^= uint64(ghash.JS(buffer))
		acc ^= ghash.JS64(buffer)
		acc ^= uint64(ghash.PJW(buffer))
		acc ^= ghash.PJW64(buffer)
		acc ^= uint64(ghash.RS(buffer))
		acc ^= ghash.RS64(buffer)
		acc ^= uint64(ghash.SDBM(buffer))
		acc ^= ghash.SDBM64(buffer)
		sink ^= acc
	})

	payload := make([]byte, 1024)
	for i := range payload {
		payload[i] = 0xA5
	}
	report("base64_encode_1KiB", 200000, func() {
		sink += uint64(len(gbase64.Encode(payload)))
	})

	const urlStr = "https://golang.org/x/crypto?go-get=1 +"
	report("gurl_encode", 500000, func() {
		sink += uint64(len(gurl.Encode(urlStr)))
	})
	report("gurl_parse", 500000, func() {
		m, err := gurl.ParseURL(urlStr, -1)
		if err == nil {
			sink += uint64(len(m))
		}
	})

	jsonMap := map[string]any{
		"key0": "value0",
		"key1": 1,
		"key2": "value2",
		"key3": 3,
		"key4": "value4",
		"key5": 5,
		"key6": "value6",
		"key7": 7,
	}
	report("json_marshal_map", 200000, func() {
		b, err := json.Marshal(jsonMap)
		if err == nil {
			sink += uint64(len(b))
		}
	})

	const jsonBytes = `{"key0":"value0","key1":1,"key2":"value2","key3":3,"key4":"value4","key5":5,"key6":"value6","key7":7}`
	report("json_unmarshal_map", 200000, func() {
		var v map[string]any
		if err := json.Unmarshal([]byte(jsonBytes), &v); err == nil {
			sink += uint64(len(v))
		}
	})

	report("md5_1KiB", 200000, func() {
		sink += uint64(len(gmd5.MustEncryptBytes(payload)))
	})
	report("sha256_1KiB", 200000, func() {
		sink += uint64(len(gsha256.Encrypt(payload)))
	})

	const htmlSrc = `<p>Test paragraph.</p><!-- Comment -->  <a href="#fragment">Other text</a>`
	report("ghtml_strip_tags", 200000, func() {
		sink += uint64(len(ghtml.StripTags(htmlSrc)))
	})
	const htmlEntities = `A 'quote' "is" <b>bold</b>`
	report("ghtml_entities", 200000, func() {
		sink += uint64(len(ghtml.Entities(htmlEntities)))
	})

	const tomlDoc = "viewpath = \"/home/www/templates/\"\n[redis]\ndisk = \"127.0.0.1:6379,0\"\ncache = \"127.0.0.1:6379,1\"\n"
	report("gtoml_decode", 100000, func() {
		v, err := gtoml.Decode([]byte(tomlDoc))
		if err == nil {
			sink++
			_ = v
		}
	})
	const yamlDoc = "url: https://goframe.org\nserver:\n  - 120.168.117.21\n  - 120.168.117.22\npi: 3.14\nhasChild: true\nname: '你好YAML'\n"
	report("gyaml_decode", 100000, func() {
		v, err := gyaml.Decode([]byte(yamlDoc))
		if err == nil {
			sink += uint64(len(v))
		}
	})

	report("gipv4_intranet", 1000000, func() {
		if gipv4.IsIntranet("192.168.1.1") {
			sink++
		}
	})
	report("gregex_match", 200000, func() {
		if gregex.IsMatchString(`(.+):(\d+)`, "sfs:2323") {
			sink++
		}
	})

	report("gcrc32_1KiB", 200000, func() {
		sink += uint64(gcrc32.Encrypt(payload))
	})
	report("grand_intn", 2000000, func() {
		sink += uint64(grand.Intn(1000))
	})
	report("gbinary_encode", 500000, func() {
		sink += uint64(len(gbinary.Encode(123, uint(456), "x")))
	})
	const propertiesDoc = "viewpath = \"/home/www/templates/\"\nredis.disk = \"127.0.0.1:6379,0\"\nredis.cache = \"127.0.0.1:6379,1\"\n"
	report("gproperties_decode", 200000, func() {
		m, err := gproperties.Decode([]byte(propertiesDoc))
		if err == nil {
			sink += uint64(len(m))
		}
	})
	aesKey := []byte("1234567891234567")
	report("gaes_cbc_encrypt", 100000, func() {
		b, err := gaes.Encrypt([]byte("pibigstar"), aesKey)
		if err == nil {
			sink += uint64(len(b))
		}
	})
	aesCipherText, _ := gaes.Encrypt([]byte("pibigstar"), aesKey)
	report("gaes_cbc_decrypt", 100000, func() {
		b, err := gaes.Decrypt(aesCipherText, aesKey)
		if err == nil {
			sink += uint64(len(b))
		}
	})

	const xmlDoc = "<root><bool>true</bool><float>100.92</float><int>123</int><string>hello world</string></root>"
	report("gxml_decode", 200000, func() {
		m, err := gxml.Decode([]byte(xmlDoc))
		if err == nil {
			sink += uint64(len(m))
		}
	})
	xmlMap, _ := gxml.Decode([]byte(xmlDoc))
	report("gxml_encode", 200000, func() {
		b, err := gxml.Encode(xmlMap)
		if err == nil {
			sink += uint64(len(b))
		}
	})

	report("gzip_1KiB", 100000, func() {
		b, err := gcompress.Gzip(payload)
		if err == nil {
			sink += uint64(len(b))
		}
	})
	gzipped, _ := gcompress.Gzip(payload)
	report("gunzip_1KiB", 100000, func() {
		b, err := gcompress.UnGzip(gzipped)
		if err == nil {
			sink += uint64(len(b))
		}
	})

	gbkBytes := []byte{0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0xb3, 0xa3, 0xd3, 0xc3, 0x87, 0xf8, 0xd7, 0xd6, 0x98, 0xcb, 0x9c, 0xca, 0xd7, 0xd6, 0xf3, 0x77, 0xb1, 0xed}
	report("gcharset_gbk_to_utf8", 200000, func() {
		s, err := gcharset.Convert("UTF-8", "gbk", string(gbkBytes))
		if err == nil {
			sink += uint64(len(s))
		}
	})

	const gjsonDoc = `{"users":[{"name":"john","score":90},{"name":"jane","score":80}],"meta":{"count":2}}`
	report("gconv_int", 1000000, func() {
		sink += uint64(gconv.Int64("123456"))
	})
	jsonDoc := gjson.New(gjsonDoc)
	report("gjson_get", 1000000, func() {
		sink += uint64(len(jsonDoc.Get("users.0.name").String()))
	})
	report("gjson_new", 200000, func() {
		_ = gjson.New(gjsonDoc)
		sink++
	})

	report("gstr_len_rune", 2000000, func() {
		sink += uint64(gstr.LenRune("Hello 常用國字標準字體表"))
	})
	report("gstr_contains_i", 1000000, func() {
		if gstr.ContainsI("GoFrame Framework", "framework") {
			sink++
		}
	})
	report("gstr_split_join", 500000, func() {
		sink += uint64(len(gstr.Join(gstr.Split("a,b,c,d,e", ","), "-")))
	})
	report("gstr_is_sub_domain", 1000000, func() {
		if gstr.IsSubDomain("s.goframe.org", "goframe.org") {
			sink++
		}
	})
	report("gstr_uc_words", 500000, func() {
		sink += uint64(len(gstr.UcWords("hello world from goframe")))
	})

	report("gstr_case_snake", 500000, func() {
		sink += uint64(len(gstr.CaseSnake("AnyKindOfString")))
	})
	report("gstr_number_format", 500000, func() {
		sink += uint64(len(gstr.NumberFormat(1234.56, 2, ",", " ")))
	})
	report("gstr_word_wrap", 300000, func() {
		sink += uint64(len(gstr.WordWrap("hello world from goframe", 10, "\n")))
	})
	report("gstr_parse", 300000, func() {
		m, err := gstr.Parse("v[a]=m&v[b]=n")
		if err == nil {
			sink += uint64(len(m))
		}
	})
	report("gstr_compare_version", 1000000, func() {
		sink += uint64(gstr.CompareVersion("v1.0.1", "v1.0.2") + 2)
	})
	report("gstr_similar_text", 200000, func() {
		sink += uint64(gstr.SimilarText("Hello", "World", nil))
	})

	report("gvar_int", 1000000, func() {
		v := gvar.New(123)
		sink += uint64(v.Int())
	})
	report("gmap_set_get", 300000, func() {
		m := gmap.New(true)
		m.Set("k", 1)
		sink += uint64(m.Get("k").(int))
	})
	report("gdebug_caller", 10000, func() {
		function, _, _ := gdebug.Caller()
		sink += uint64(len(function))
	})

	report("garray_append_at", 300000, func() {
		a := garray.New()
		a.Append(1, 2, 3)
		sink += uint64(a.At(2).(int))
	})
	report("gset_add_contains", 300000, func() {
		set := gset.NewIntSet()
		set.Add(1, 2, 3)
		if set.Contains(2) {
			sink++
		}
	})
	report("glist_push_pop", 500000, func() {
		l := glist.New()
		l.PushBack(1)
		sink += uint64(l.PopFront().(int))
	})
	report("gring_put_slice", 300000, func() {
		r := gring.New(4)
		r.Put(1)
		r.Put(2)
		sink += uint64(len(r.SliceNext()))
	})
	benchQueue := gqueue.New()
	report("gqueue_push_pop", 300000, func() {
		benchQueue.Push(1)
		sink += uint64(benchQueue.Pop().(int))
	})
	benchQueue.Close()

	report("runtime_callers", 100000, func() {
		var pcs [64]uintptr
		sink += uint64(runtime.Callers(3, pcs[:]))
	})

	report("gerror_new_1", 200000, func() {
		err := gerror.New("base")
		if err != nil {
			sink++
		}
	})

	report("gerror_create_chain3", 100000, func() {
		err := gerror.New("base")
		err = gerror.Wrap(err, "mid")
		err = gerror.Wrap(err, "top")
		sink += uint64(len(err.Error()))
	})

	var chain = func() error {
		err := gerror.New("base")
		err = gerror.Wrap(err, "mid")
		return gerror.Wrap(err, "top")
	}()
	report("gerror_render_chain3", 500000, func() {
		sink += uint64(len(chain.Error()))
	})

	report("gerror_newf_format", 200000, func() {
		sink += uint64(len(gerror.Newf("value=%s count=%d", "value", 42).Error()))
	})

	report("utils_strings", 200000, func() {
		removed := utils.RemoveSymbols("my_custom_key-42 value")
		numeric := utils.IsNumeric("123.45")
		trimmed := utils.Trim("---my_custom_key---", "-")
		formatted := utils.FormatCmdKey("MY_CUSTOM_KEY")
		if numeric {
			sink += uint64(len(removed) + len(trimmed) + len(formatted))
		}
	})

	report("command_parse", 200000, func() {
		parsedArgs, parsedOptions := command.ParseUsingDefaultAlgorithm(
			"gf", "gen", "-n", "2", "--name=x", "-d", "-h",
		)
		sink += uint64(len(parsedArgs) + len(parsedOptions))
	})

	nonEmptyMap := map[string]any{"k": 1}
	emptyMap := map[string]any{}
	report("empty_checks", 2000000, func() {
		var hits uint64
		if empty.IsEmpty(0) {
			hits++
		}
		if empty.IsEmpty("") {
			hits++
		}
		if empty.IsEmpty(emptyMap) {
			hits++
		}
		if empty.IsEmpty(nonEmptyMap) {
			hits++
		}
		sink += hits
	})

	report("gtype_set_val_add_cas", 1000000, func() {
		v := gtype.NewInt(0)
		v.Set(1)
		v.Add(1)
		v.Cas(2, 3)
		sink += uint64(v.Val())
	})
	report("gtype_any_set_val", 1000000, func() {
		v := gtype.New(123)
		v.Set(1)
		sink += uint64(v.Val().(int))
	})

	report("gsorted_int_add_search", 300000, func() {
		a := garray.NewSortedIntArray()
		a.Add(1, 2, 3)
		sink += uint64(a.Search(2))
	})

	report("gmap_list_set_get", 300000, func() {
		m := gmap.NewListKVMap[string, any]()
		m.Set("k", 1)
		if m.Get("k") != nil {
			sink++
		}
	})
	iterateList := gmap.NewListMap()
	for i := 0; i < 100; i++ {
		iterateList.Set(fmt.Sprintf("k%d", i), i)
	}
	report("gmap_list_iterate_100", 100000, func() {
		count := 0
		iterateList.Iterator(func(key, value any) bool {
			count++
			return true
		})
		sink += uint64(count)
	})
	report("gmap_tree_set_get", 300000, func() {
		m := gmap.NewTreeMap(gutil.ComparatorString)
		m.Set("k", 1)
		if m.Get("k") != nil {
			sink++
		}
	})
	report("gtree_set_get", 300000, func() {
		tree := gtree.NewRedBlackTree(gutil.ComparatorString)
		tree.Set("k", 1)
		if tree.Get("k") != nil {
			sink++
		}
	})
	iterateTree := gtree.NewRedBlackTree(gutil.ComparatorInt)
	for i := 0; i < 100; i++ {
		iterateTree.Set(i, i)
	}
	report("gtree_iterate_100", 200000, func() {
		count := 0
		iterateTree.Iterator(func(key, value any) bool {
			count++
			return true
		})
		sink += uint64(count)
	})

	report("gpage_page_bar", 500000, func() {
		page := gpage.New(100, 10, 5, "/user/list?page={.page}")
		sink += uint64(len(page.PageBar()))
	})
	report("guid_s", 200000, func() {
		sink += uint64(len(guid.S()))
	})

	envEntries := []string{"a=1", "b=2"}
	report("genv_map_from_env", 500000, func() {
		sink += uint64(len(genv.MapFromEnv(envEntries)))
	})

	benchTime := gtime.NewFromTimeStamp(1800000000)
	report("gtime_format", 1000000, func() {
		sink += uint64(len(benchTime.Format("Y-m-d H:i:s")))
	})
	report("gtime_str_to_time", 200000, func() {
		t, err := gtime.StrToTime("2018-07-18 12:01:02")
		if err == nil {
			sink += uint64(t.Timestamp())
		}
	})

	fmt.Println("sink", sink)
}
