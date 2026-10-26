package main

import (
	"encoding/binary"

	"github.com/oriumgames/nbt"
	"github.com/oriumgames/schem/format"
)

// patchAxiomEntities restores entity positions, rotations and motion that the
// library reader drops. The library asserts concrete slice types on values
// decoded into map[string]any, which never match, so those fields arrive zero.
// Reading the blueprint's data section a second time is cheap and the patch is
// best-effort: a failure leaves the library result untouched.
func patchAxiomEntities(s format.Schematic, data []byte) format.Schematic {
	entities, ok := axiomEntityData(data)
	if !ok {
		return s
	}
	targets := s.Entities()
	if len(entities) != len(targets) {
		return s
	}

	offsetX, offsetY, offsetZ := s.Offset()
	for i, raw := range entities {
		entry, ok := asMap(raw)
		if !ok {
			continue
		}
		if pos, ok := asFloat3(entry["Pos"]); ok {
			targets[i].Pos = [3]float64{
				pos[0] - float64(offsetX),
				pos[1] - float64(offsetY),
				pos[2] - float64(offsetZ),
			}
		}
		if rotation, ok := asFloat2(entry["Rotation"]); ok {
			targets[i].Rotation = rotation
		}
		if motion, ok := asFloat3(entry["Motion"]); ok {
			targets[i].Motion = motion
		}
	}
	return s
}

func axiomEntityData(data []byte) ([]any, bool) {
	if len(data) < 8 || binary.BigEndian.Uint32(data[:4]) != axiomMagic {
		return nil, false
	}

	offset := 4
	headerLen := binary.BigEndian.Uint32(data[offset:])
	offset += 4
	if uint64(offset)+uint64(headerLen)+4 > uint64(len(data)) {
		return nil, false
	}
	offset += int(headerLen)

	thumbnailLen := binary.BigEndian.Uint32(data[offset:])
	offset += 4
	if uint64(offset)+uint64(thumbnailLen)+4 > uint64(len(data)) {
		return nil, false
	}
	offset += int(thumbnailLen)

	dataLen := binary.BigEndian.Uint32(data[offset:])
	offset += 4
	if uint64(offset)+uint64(dataLen) > uint64(len(data)) {
		return nil, false
	}

	decompressed, ok := gunzip(data[offset : offset+int(dataLen)])
	if !ok {
		return nil, false
	}
	var root map[string]any
	if err := nbt.UnmarshalEncoding(decompressed, &root, nbt.BigEndian); err != nil {
		return nil, false
	}
	entities, ok := asAnySlice(root["Entities"])
	if !ok {
		return nil, false
	}
	return entities, true
}
