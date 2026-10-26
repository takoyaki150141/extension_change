package main

import (
	"bytes"
	"compress/gzip"
	"fmt"
	"io"
	"reflect"
	"strings"

	"github.com/oriumgames/nbt"
	"github.com/oriumgames/schem/format"
)

// spongeV3Document mirrors enough of the Sponge v3 layout to re-encode a
// document while preserving every field. Unknown fields are kept in Extra.
type spongeV3Document struct {
	Schematic spongeV3Schematic `nbt:"Schematic"`
}

type spongeV3Schematic struct {
	Blocks spongeV3Blocks `nbt:"Blocks"`
	Extra  map[string]any `nbt:"*"`
}

type spongeV3Blocks struct {
	BlockEntities []map[string]any `nbt:"BlockEntities,omitempty"`
	Data          any              `nbt:"Data,omitempty"`
	Extra         map[string]any   `nbt:"*"`
}

func detectNestedSpongeV3(root map[string]any) string {
	schematic, ok := asMap(root["Schematic"])
	if !ok {
		return ""
	}
	version, ok := asInt(schematic["Version"])
	if !ok {
		return ""
	}
	if version == 3 {
		return "sponge_v3"
	}
	return ""
}

// writeSpongeV3 writes a spec-conformant Sponge v3 file. The library writes
// several array fields as TAG_List and puts block entity and entity data at the
// top level; the spec asks for TAG_IntArray/TAG_ByteArray and a Data compound.
// It also reads block entity positions as TAG_IntArray.
func writeSpongeV3(w io.Writer, s format.Schematic) error {
	var raw bytes.Buffer
	if err := format.WriteFormat(&raw, "sponge_v3", s); err != nil {
		return fmt.Errorf("encode Sponge v3: %w", err)
	}
	normalized, err := normalizeSpongeV3ForOutput(raw.Bytes())
	if err != nil {
		return fmt.Errorf("normalize Sponge v3: %w", err)
	}
	if _, err := w.Write(normalized); err != nil {
		return fmt.Errorf("write Sponge v3: %w", err)
	}
	return nil
}

// spongeV3ForLibrary converts a Sponge v3 document into the shape the library
// reader accepts: TAG_List for Offset and byte data, block entity and entity
// data inlined, and block entity positions as TAG_IntArray.
func spongeV3ForLibrary(data []byte) ([]byte, bool) {
	doc, err := decodeSpongeV3(data)
	if err != nil {
		return nil, false
	}

	if offset, ok := asInt3(doc.Schematic.Extra["Offset"]); ok {
		doc.Schematic.Extra["Offset"] = []int32{offset[0], offset[1], offset[2]}
	}
	if data, ok := byteArrayToBytes(doc.Schematic.Blocks.Data); ok {
		doc.Schematic.Blocks.Data = data
	}
	modifyBiomeData(doc.Schematic.Extra, byteArrayToBytes)

	if len(doc.Schematic.Blocks.BlockEntities) > 0 {
		out := make([]map[string]any, 0, len(doc.Schematic.Blocks.BlockEntities))
		for _, raw := range doc.Schematic.Blocks.BlockEntities {
			pos, ok := asInt3(raw["Pos"])
			if !ok {
				pos, ok = int3FromXYZ(raw)
			}
			if !ok {
				pos = [3]int32{}
			}
			id, _ := asString(raw["Id"])
			if id == "" {
				id, _ = asString(raw["id"])
			}

			entry := map[string]any{"Pos": pos, "Id": id}
			if rawData, ok := asMap(raw["Data"]); ok {
				for k, v := range rawData {
					if k != "Pos" && k != "Id" {
						entry[k] = deepCopyValue(v)
					}
				}
			}
			for k, v := range raw {
				switch strings.ToLower(k) {
				case "pos", "id", "data", "x", "y", "z":
					continue
				}
				entry[k] = deepCopyValue(v)
			}
			out = append(out, entry)
		}
		doc.Schematic.Blocks.BlockEntities = out
	}

	if rawEntities, ok := doc.Schematic.Extra["Entities"]; ok {
		if entities, ok := asAnySlice(rawEntities); ok {
			out := make([]any, 0, len(entities))
			for _, rawEntity := range entities {
				entity, ok := asMap(rawEntity)
				if !ok {
					out = append(out, rawEntity)
					continue
				}
				pos, ok := asFloat3(entity["Pos"])
				if !ok {
					pos = [3]float64{}
				}
				id, _ := asString(entity["Id"])
				if id == "" {
					id, _ = asString(entity["id"])
				}

				entry := map[string]any{
					"Pos": []float64{pos[0], pos[1], pos[2]},
					"Id":  id,
				}
				if rawData, ok := asMap(entity["Data"]); ok {
					for k, v := range rawData {
						if k != "Pos" && k != "Id" {
							entry[k] = deepCopyValue(v)
						}
					}
				}
				for k, v := range entity {
					switch strings.ToLower(k) {
					case "pos", "id", "data":
						continue
					}
					entry[k] = deepCopyValue(v)
				}
				out = append(out, entry)
			}
			doc.Schematic.Extra["Entities"] = out
		}
	}

	out, err := encodeSpongeV3(doc)
	if err != nil {
		return nil, false
	}
	return out, true
}

// normalizeSpongeV3ForOutput converts library output into spec-conformant NBT.
func normalizeSpongeV3ForOutput(data []byte) ([]byte, error) {
	doc, err := decodeSpongeV3(data)
	if err != nil {
		return nil, err
	}

	if offset, ok := asInt3(doc.Schematic.Extra["Offset"]); ok {
		doc.Schematic.Extra["Offset"] = offset
	}
	if data, ok := bytesToByteArray(doc.Schematic.Blocks.Data); ok {
		doc.Schematic.Blocks.Data = data
	}
	modifyBiomeData(doc.Schematic.Extra, bytesToByteArray)

	if len(doc.Schematic.Blocks.BlockEntities) > 0 {
		out := make([]map[string]any, 0, len(doc.Schematic.Blocks.BlockEntities))
		for _, raw := range doc.Schematic.Blocks.BlockEntities {
			pos, ok := asInt3(raw["Pos"])
			if !ok {
				pos, ok = int3FromXYZ(raw)
			}
			if !ok {
				pos = [3]int32{}
			}
			id, _ := asString(raw["Id"])
			if id == "" {
				id, _ = asString(raw["id"])
			}

			extra := cloneMap(raw)
			deleteKeysFold(extra, "pos", "id", "x", "y", "z")
			entry := map[string]any{"Pos": pos, "Id": id}
			if len(extra) > 0 {
				entry["Data"] = extra
			}
			out = append(out, entry)
		}
		doc.Schematic.Blocks.BlockEntities = out
	}

	if rawEntities, ok := doc.Schematic.Extra["Entities"]; ok {
		if entities, ok := asAnySlice(rawEntities); ok {
			out := make([]any, 0, len(entities))
			for _, rawEntity := range entities {
				entity, ok := asMap(rawEntity)
				if !ok {
					out = append(out, rawEntity)
					continue
				}
				pos, ok := asFloat3(entity["Pos"])
				if !ok {
					pos = [3]float64{}
				}
				id, _ := asString(entity["Id"])
				if id == "" {
					id, _ = asString(entity["id"])
				}

				extra := cloneMap(entity)
				deleteKeysFold(extra, "pos", "id")
				entry := map[string]any{
					"Pos": []float64{pos[0], pos[1], pos[2]},
					"Id":  id,
				}
				if len(extra) > 0 {
					entry["Data"] = extra
				}
				out = append(out, entry)
			}
			doc.Schematic.Extra["Entities"] = out
		}
	}

	return encodeSpongeV3(doc)
}

func decodeSpongeV3(data []byte) (*spongeV3Document, error) {
	gz, err := gzip.NewReader(bytes.NewReader(data))
	if err != nil {
		return nil, fmt.Errorf("decompress: %w", err)
	}
	decompressed, err := io.ReadAll(gz)
	gz.Close()
	if err != nil {
		return nil, fmt.Errorf("decompress: %w", err)
	}

	var doc spongeV3Document
	if err := nbt.UnmarshalEncoding(decompressed, &doc, nbt.BigEndian); err != nil {
		return nil, fmt.Errorf("decode NBT: %w", err)
	}
	return &doc, nil
}

func encodeSpongeV3(doc *spongeV3Document) ([]byte, error) {
	var out bytes.Buffer
	gz := gzip.NewWriter(&out)
	if err := nbt.NewEncoderWithEncoding(gz, nbt.BigEndian).Encode(doc); err != nil {
		gz.Close()
		return nil, fmt.Errorf("encode NBT: %w", err)
	}
	if err := gz.Close(); err != nil {
		return nil, fmt.Errorf("compress: %w", err)
	}
	return out.Bytes(), nil
}

func modifyBiomeData(extra map[string]any, convert func(any) (any, bool)) {
	rawBiomes, ok := extra["Biomes"]
	if !ok {
		return
	}
	biomes, ok := asMap(rawBiomes)
	if !ok {
		return
	}
	if rawData, ok := biomes["Data"]; ok {
		if converted, ok := convert(rawData); ok {
			biomes["Data"] = converted
		}
	}
}

func int3FromXYZ(m map[string]any) ([3]int32, bool) {
	x, okX := asInt(m["x"])
	y, okY := asInt(m["y"])
	z, okZ := asInt(m["z"])
	if !okX || !okY || !okZ {
		return [3]int32{}, false
	}
	return [3]int32{int32(x), int32(y), int32(z)}, true
}

func bytesToByteArray(v any) (any, bool) {
	if v == nil {
		return nil, false
	}
	values, ok := asAnySlice(v)
	if !ok {
		return nil, false
	}
	array := reflect.New(reflect.ArrayOf(len(values), reflect.TypeOf(byte(0)))).Elem()
	for i, item := range values {
		n, ok := asInt(item)
		if !ok {
			return nil, false
		}
		array.Index(i).SetUint(uint64(n))
	}
	return array.Interface(), true
}

func byteArrayToBytes(v any) (any, bool) {
	if v == nil {
		return nil, false
	}
	values, ok := asAnySlice(v)
	if !ok {
		return nil, false
	}
	out := make([]byte, len(values))
	for i, item := range values {
		n, ok := asInt(item)
		if !ok {
			return nil, false
		}
		out[i] = byte(n)
	}
	return out, true
}
