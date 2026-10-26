package main

import (
	"bytes"
	"compress/gzip"
	"encoding/binary"
	"fmt"
	"io"
	"reflect"

	"github.com/oriumgames/nbt"
	"github.com/oriumgames/schem/format"
)

// noBlockEntities hides block entities from the library's Axiom writer. The
// writer puts block entity coordinates into map[string]any as plain int, which
// the NBT encoder rejects. They are added back after the writer has run.
type noBlockEntities struct {
	format.Schematic
}

func (s *noBlockEntities) BlockEntity(x, y, z int) *format.BlockEntity {
	return nil
}

func writeAxiom(w io.Writer, s format.Schematic) error {
	var raw bytes.Buffer
	if err := format.WriteFormat(&raw, "axiom", &noBlockEntities{Schematic: s}); err != nil {
		return fmt.Errorf("encode Axiom blueprint: %w", err)
	}
	patched, err := patchAxiomOutput(raw.Bytes(), s)
	if err != nil {
		return fmt.Errorf("finalize Axiom blueprint: %w", err)
	}
	if _, err := w.Write(patched); err != nil {
		return fmt.Errorf("write Axiom blueprint: %w", err)
	}
	return nil
}

func patchAxiomOutput(data []byte, s format.Schematic) ([]byte, error) {
	prefix, compressed, ok := splitAxiomData(data)
	if !ok {
		return nil, fmt.Errorf("invalid Axiom container")
	}
	decompressed, ok := gunzip(compressed)
	if !ok {
		return nil, fmt.Errorf("decompress block data")
	}

	var root map[string]any
	if err := nbt.UnmarshalEncoding(decompressed, &root, nbt.BigEndian); err != nil {
		return nil, fmt.Errorf("decode block data: %w", err)
	}

	if regions, ok := asAnySlice(root["BlockRegion"]); ok {
		for _, rawRegion := range regions {
			region, ok := asMap(rawRegion)
			if !ok {
				continue
			}
			for _, key := range []string{"BlockStates", "data"} {
				states, ok := asMap(region[key])
				if !ok {
					continue
				}
				if longs, ok := toLongArray(states["Data"]); ok {
					states["Data"] = longs
				}
			}
		}
	}

	offsetX, offsetY, offsetZ := s.Offset()
	blockEntities := make([]any, 0)
	width, height, length := s.Dimensions()
	for y := 0; y < height; y++ {
		for z := 0; z < length; z++ {
			for x := 0; x < width; x++ {
				be := s.BlockEntity(x, y, z)
				if be == nil {
					continue
				}
				entry := cloneMap(be.Data)
				deleteKeysFold(entry, "x", "y", "z", "id")
				entry["x"] = int32(x + offsetX)
				entry["y"] = int32(y + offsetY)
				entry["z"] = int32(z + offsetZ)
				if be.ID != "" {
					entry["id"] = be.ID
				}
				blockEntities = append(blockEntities, sanitizeNBT(entry))
			}
		}
	}
	if len(blockEntities) > 0 {
		root["BlockEntities"] = blockEntities
	}

	var encoded bytes.Buffer
	gz := gzip.NewWriter(&encoded)
	if err := nbt.NewEncoderWithEncoding(gz, nbt.BigEndian).Encode(root); err != nil {
		gz.Close()
		return nil, fmt.Errorf("encode block data: %w", err)
	}
	if err := gz.Close(); err != nil {
		return nil, fmt.Errorf("compress block data: %w", err)
	}

	var out bytes.Buffer
	out.Write(prefix)
	if err := binary.Write(&out, binary.BigEndian, uint32(encoded.Len())); err != nil {
		return nil, err
	}
	out.Write(encoded.Bytes())
	return out.Bytes(), nil
}

// splitAxiomData returns the bytes up to and including the data length prefix
// (which the caller rewrites), and the compressed data section.
func splitAxiomData(data []byte) ([]byte, []byte, bool) {
	if len(data) < 8 || binary.BigEndian.Uint32(data[:4]) != axiomMagic {
		return nil, nil, false
	}
	offset := 4

	headerLen := binary.BigEndian.Uint32(data[offset:])
	offset += 4
	if uint64(offset)+uint64(headerLen)+4 > uint64(len(data)) {
		return nil, nil, false
	}
	offset += int(headerLen)

	thumbnailLen := binary.BigEndian.Uint32(data[offset:])
	offset += 4
	if uint64(offset)+uint64(thumbnailLen)+4 > uint64(len(data)) {
		return nil, nil, false
	}
	offset += int(thumbnailLen)

	prefixLen := offset
	dataLen := binary.BigEndian.Uint32(data[offset:])
	offset += 4
	if uint64(offset)+uint64(dataLen) > uint64(len(data)) {
		return nil, nil, false
	}
	return data[:prefixLen], data[offset : offset+int(dataLen)], true
}

func toLongArray(v any) (any, bool) {
	if v == nil {
		return nil, false
	}
	rv := reflect.ValueOf(v)
	if rv.Kind() == reflect.Array && rv.Type().Elem().Kind() == reflect.Int64 {
		return v, true
	}
	values, ok := asAnySlice(v)
	if !ok || len(values) == 0 {
		return nil, false
	}
	array := reflect.New(reflect.ArrayOf(len(values), reflect.TypeOf(int64(0)))).Elem()
	for i, item := range values {
		n, ok := asInt(item)
		if !ok {
			return nil, false
		}
		array.Index(i).SetInt(int64(n))
	}
	return array.Interface(), true
}
