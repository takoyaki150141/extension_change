package main

import (
	"bytes"
	"compress/gzip"
	"compress/zlib"
	"fmt"
	"io"
	"reflect"
	"sort"
	"strings"

	"github.com/oriumgames/nbt"
)

func tryReadRootNBT(data []byte) (map[string]any, bool) {
	raw, ok := maybeDecompressNBT(data)
	if !ok {
		return nil, false
	}
	var root map[string]any
	if err := nbt.UnmarshalEncoding(raw, &root, nbt.BigEndian); err != nil {
		return nil, false
	}
	return root, true
}

func maybeDecompressNBT(data []byte) ([]byte, bool) {
	switch {
	case len(data) >= 2 && data[0] == 0x1F && data[1] == 0x8B:
		return gunzip(data)
	case len(data) >= 2 && data[0] == 0x78:
		return unzlib(data)
	case len(data) >= 1 && data[0] == 0x0A:
		return data, true
	default:
		return nil, false
	}
}

func gunzip(data []byte) ([]byte, bool) {
	r, err := gzip.NewReader(bytes.NewReader(data))
	if err != nil {
		return nil, false
	}
	defer r.Close()
	out, err := io.ReadAll(r)
	if err != nil {
		return nil, false
	}
	return out, true
}

func unzlib(data []byte) ([]byte, bool) {
	r, err := zlib.NewReader(bytes.NewReader(data))
	if err != nil {
		return nil, false
	}
	defer r.Close()
	out, err := io.ReadAll(r)
	if err != nil {
		return nil, false
	}
	return out, true
}

func asInt(v any) (int, bool) {
	switch n := v.(type) {
	case int:
		return n, true
	case int8:
		return int(n), true
	case int16:
		return int(n), true
	case int32:
		return int(n), true
	case int64:
		return int(n), true
	case uint8:
		return int(n), true
	case uint16:
		return int(n), true
	case uint32:
		return int(n), true
	case uint64:
		return int(n), true
	default:
		return 0, false
	}
}

func asFloat(v any) (float64, bool) {
	switch n := v.(type) {
	case float32:
		return float64(n), true
	case float64:
		return n, true
	default:
		i, ok := asInt(v)
		return float64(i), ok
	}
}

func asString(v any) (string, bool) {
	s, ok := v.(string)
	return s, ok
}

func asMap(v any) (map[string]any, bool) {
	switch m := v.(type) {
	case map[string]any:
		return m, true
	case map[string]string:
		out := make(map[string]any, len(m))
		for k, val := range m {
			out[k] = val
		}
		return out, true
	}
	rv := reflect.ValueOf(v)
	if !rv.IsValid() || rv.Kind() != reflect.Map || rv.Type().Key().Kind() != reflect.String {
		return nil, false
	}
	out := make(map[string]any, rv.Len())
	iter := rv.MapRange()
	for iter.Next() {
		out[iter.Key().String()] = iter.Value().Interface()
	}
	return out, true
}

func asAnySlice(v any) ([]any, bool) {
	if v == nil {
		return nil, false
	}
	if s, ok := v.([]any); ok {
		return s, true
	}
	if s, ok := v.([]byte); ok {
		out := make([]any, len(s))
		for i, b := range s {
			out[i] = b
		}
		return out, true
	}
	rv := reflect.ValueOf(v)
	if rv.Kind() != reflect.Slice && rv.Kind() != reflect.Array {
		return nil, false
	}
	out := make([]any, rv.Len())
	for i := 0; i < rv.Len(); i++ {
		out[i] = rv.Index(i).Interface()
	}
	return out, true
}

func asInt3(v any) ([3]int32, bool) {
	values, ok := asAnySlice(v)
	if !ok || len(values) < 3 {
		return [3]int32{}, false
	}
	var out [3]int32
	for i := 0; i < 3; i++ {
		n, ok := asInt(values[i])
		if !ok {
			return [3]int32{}, false
		}
		out[i] = int32(n)
	}
	return out, true
}

func asFloat3(v any) ([3]float64, bool) {
	values, ok := asAnySlice(v)
	if !ok || len(values) < 3 {
		return [3]float64{}, false
	}
	var out [3]float64
	for i := 0; i < 3; i++ {
		f, ok := asFloat(values[i])
		if !ok {
			return [3]float64{}, false
		}
		out[i] = f
	}
	return out, true
}

func asFloat2(v any) ([2]float32, bool) {
	values, ok := asAnySlice(v)
	if !ok || len(values) < 2 {
		return [2]float32{}, false
	}
	var out [2]float32
	for i := 0; i < 2; i++ {
		f, ok := asFloat(values[i])
		if !ok {
			return [2]float32{}, false
		}
		out[i] = float32(f)
	}
	return out, true
}

func asUUID(v any) ([4]int32, bool) {
	values, ok := asAnySlice(v)
	if !ok || len(values) < 4 {
		return [4]int32{}, false
	}
	var out [4]int32
	for i := 0; i < 4; i++ {
		n, ok := asInt(values[i])
		if !ok {
			return [4]int32{}, false
		}
		out[i] = int32(n)
	}
	return out, true
}

func cloneMap(m map[string]any) map[string]any {
	if m == nil {
		return make(map[string]any)
	}
	out := make(map[string]any, len(m))
	for k, v := range m {
		out[k] = deepCopyValue(v)
	}
	return out
}

func deepCopyValue(v any) any {
	switch x := v.(type) {
	case map[string]any:
		return cloneMap(x)
	case []any:
		out := make([]any, len(x))
		for i, item := range x {
			out[i] = deepCopyValue(item)
		}
		return out
	case []byte:
		out := make([]byte, len(x))
		copy(out, x)
		return out
	}

	rv := reflect.ValueOf(v)
	if rv.IsValid() && rv.Kind() == reflect.Slice {
		out := reflect.MakeSlice(rv.Type(), rv.Len(), rv.Len())
		reflect.Copy(out, rv)
		return out.Interface()
	}
	return v
}

func deleteKeysFold(m map[string]any, keys ...string) {
	for k := range m {
		for _, key := range keys {
			if strings.EqualFold(k, key) {
				delete(m, k)
				break
			}
		}
	}
}

func findKeyFold(m map[string]any, key string) (any, bool) {
	for k, v := range m {
		if strings.EqualFold(k, key) {
			return v, true
		}
	}
	return nil, false
}

func sanitizeNBT(v any) any {
	switch x := v.(type) {
	case nil:
		return nil
	case int:
		return int32(x)
	case uint:
		return int32(x)
	case uint16:
		return int16(x)
	case uint32:
		return int32(x)
	case uint64:
		return int64(x)
	case map[string]any:
		out := make(map[string]any, len(x))
		for k, item := range x {
			out[k] = sanitizeNBT(item)
		}
		return out
	case []any:
		out := make([]any, len(x))
		for i, item := range x {
			out[i] = sanitizeNBT(item)
		}
		return out
	case []int:
		out := make([]int32, len(x))
		for i, item := range x {
			out[i] = int32(item)
		}
		return out
	default:
		return v
	}
}

func blockStateKey(name string, props map[string]any) string {
	if len(props) == 0 {
		return name
	}
	keys := make([]string, 0, len(props))
	for k := range props {
		keys = append(keys, k)
	}
	sort.Strings(keys)

	var b strings.Builder
	b.WriteString(name)
	b.WriteByte('[')
	for i, k := range keys {
		if i > 0 {
			b.WriteByte(',')
		}
		b.WriteString(k)
		b.WriteByte('=')
		fmt.Fprint(&b, props[k])
	}
	b.WriteByte(']')
	return b.String()
}
