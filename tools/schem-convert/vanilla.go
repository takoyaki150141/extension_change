package main

import (
	"compress/gzip"
	"fmt"
	"io"
	"math"

	"github.com/oriumgames/nbt"
	"github.com/oriumgames/schem/format"
)

const defaultDataVersion = 3465 // Minecraft 1.20.1

func isVanillaStructureRoot(root map[string]any) bool {
	if _, ok := asInt3(root["size"]); !ok {
		return false
	}
	if _, ok := asAnySlice(root["palette"]); !ok {
		return false
	}
	if _, ok := asAnySlice(root["blocks"]); !ok {
		return false
	}
	return true
}

func parseVanillaStructure(root map[string]any) (format.Schematic, error) {
	size, ok := asInt3(root["size"])
	if !ok {
		return nil, fmt.Errorf("missing or invalid size")
	}
	width, height, length := int(size[0]), int(size[1]), int(size[2])
	if width <= 0 || height <= 0 || length <= 0 {
		return nil, fmt.Errorf("invalid size %dx%dx%d", width, height, length)
	}

	s := newSchematic(width, height, length)
	if version, ok := asInt(root["DataVersion"]); ok {
		s.SetDataVersion(version)
	}

	paletteEntries, ok := asAnySlice(root["palette"])
	if !ok {
		return nil, fmt.Errorf("missing palette")
	}
	palette := make([]*format.BlockState, len(paletteEntries))
	for i, rawEntry := range paletteEntries {
		entry, ok := asMap(rawEntry)
		if !ok {
			return nil, fmt.Errorf("palette entry %d is not a compound", i)
		}
		name, ok := asString(entry["Name"])
		if !ok || name == "" {
			return nil, fmt.Errorf("palette entry %d has no Name", i)
		}
		state := &format.BlockState{Name: name}
		if rawProps, ok := entry["Properties"]; ok {
			props, ok := asMap(rawProps)
			if ok && len(props) > 0 {
				state.Properties = make(map[string]any, len(props))
				for k, v := range props {
					state.Properties[k] = deepCopyValue(v)
				}
			}
		}
		palette[i] = state
	}

	blockEntries, ok := asAnySlice(root["blocks"])
	if !ok {
		return nil, fmt.Errorf("missing blocks")
	}
	for i, rawBlock := range blockEntries {
		block, ok := asMap(rawBlock)
		if !ok {
			return nil, fmt.Errorf("block %d is not a compound", i)
		}
		pos, ok := asInt3(block["pos"])
		if !ok {
			return nil, fmt.Errorf("block %d has no valid pos", i)
		}
		x, y, z := int(pos[0]), int(pos[1]), int(pos[2])
		if x < 0 || x >= width || y < 0 || y >= height || z < 0 || z >= length {
			return nil, fmt.Errorf("block %d position %d,%d,%d is outside %dx%dx%d", i, x, y, z, width, height, length)
		}

		stateIndex, ok := asInt(block["state"])
		if !ok || stateIndex < 0 || stateIndex >= len(palette) {
			return nil, fmt.Errorf("block %d has invalid palette index", i)
		}
		s.SetBlock(x, y, z, palette[stateIndex].Clone())

		if rawBE, ok := block["nbt"]; ok {
			beData, ok := asMap(rawBE)
			if ok {
				s.SetBlockEntity(x, y, z, blockEntityFromNBT(beData))
			}
		}
	}

	if rawEntities, ok := asAnySlice(root["entities"]); ok {
		for i, rawEntity := range rawEntities {
			entity, err := entityFromNBT(rawEntity)
			if err != nil {
				return nil, fmt.Errorf("entity %d: %w", i, err)
			}
			s.AddEntity(entity)
		}
	}

	return s, nil
}

func blockEntityFromNBT(data map[string]any) *format.BlockEntity {
	be := &format.BlockEntity{
		Data: make(map[string]any),
	}
	if id, ok := asString(data["id"]); ok {
		be.ID = id
	} else if id, ok := asString(data["Id"]); ok {
		be.ID = id
	}
	for k, v := range data {
		switch k {
		case "id", "Id", "x", "y", "z":
			continue
		default:
			be.Data[k] = deepCopyValue(v)
		}
	}
	return be
}

func entityFromNBT(raw any) (*format.Entity, error) {
	entry, ok := asMap(raw)
	if !ok {
		return nil, fmt.Errorf("not a compound")
	}

	entity := &format.Entity{
		Data: make(map[string]any),
	}

	pos, hasPos := asFloat3(entry["pos"])
	if !hasPos {
		if blockPos, ok := asInt3(entry["blockPos"]); ok {
			pos = [3]float64{float64(blockPos[0]), float64(blockPos[1]), float64(blockPos[2])}
			hasPos = true
		}
	}
	if hasPos {
		entity.Pos = pos
	}

	if rawNBT, ok := entry["nbt"]; ok {
		nbtData, ok := asMap(rawNBT)
		if ok {
			if id, ok := asString(nbtData["id"]); ok {
				entity.ID = id
			}
			if rotation, ok := asFloat2(nbtData["Rotation"]); ok {
				entity.Rotation = rotation
			}
			if motion, ok := asFloat3(nbtData["Motion"]); ok {
				entity.Motion = motion
			}
			if uuid, ok := asUUID(nbtData["UUID"]); ok {
				entity.UUID = &uuid
			}
			for k, v := range nbtData {
				switch k {
				case "id", "Pos", "Rotation", "Motion", "UUID":
					continue
				default:
					entity.Data[k] = deepCopyValue(v)
				}
			}
		}
	}
	if entity.ID == "" {
		if id, ok := asString(entry["id"]); ok {
			entity.ID = id
		}
	}
	return entity, nil
}

type paletteBuilder struct {
	index   map[string]int32
	entries []map[string]any
}

func newPaletteBuilder() *paletteBuilder {
	p := &paletteBuilder{
		index: make(map[string]int32),
		entries: []map[string]any{
			{"Name": "minecraft:air"},
		},
	}
	p.index["minecraft:air"] = 0
	return p
}

func (p *paletteBuilder) add(state *format.BlockState) int32 {
	if state == nil || state.Name == "" {
		return 0
	}

	// Vanilla stores palette property values as strings, regardless of the
	// property's type in the block state.
	props := make(map[string]any, len(state.Properties))
	for k, v := range state.Properties {
		props[k] = fmt.Sprint(v)
	}
	key := blockStateKey(state.Name, props)
	if idx, ok := p.index[key]; ok {
		return idx
	}

	entry := map[string]any{"Name": state.Name}
	if len(props) > 0 {
		entry["Properties"] = props
	}
	idx := int32(len(p.entries))
	p.entries = append(p.entries, entry)
	p.index[key] = idx
	return idx
}

func writeVanillaNBT(w io.Writer, s format.Schematic) error {
	width, height, length := s.Dimensions()
	if width <= 0 || height <= 0 || length <= 0 {
		return fmt.Errorf("invalid dimensions %dx%dx%d for a vanilla structure", width, height, length)
	}

	palette := newPaletteBuilder()
	blocks := make([]map[string]any, 0, width*height*length)
	for y := 0; y < height; y++ {
		for z := 0; z < length; z++ {
			for x := 0; x < width; x++ {
				entry := map[string]any{
					"pos":   [3]int32{int32(x), int32(y), int32(z)},
					"state": palette.add(s.Block(x, y, z)),
				}
				if be := s.BlockEntity(x, y, z); be != nil {
					data := cloneMap(be.Data)
					deleteKeysFold(data, "x", "y", "z", "id")
					if be.ID != "" {
						data["id"] = be.ID
					}
					entry["nbt"] = sanitizeNBT(data)
				}
				blocks = append(blocks, entry)
			}
		}
	}

	entities := make([]map[string]any, 0, len(s.Entities()))
	for _, entity := range s.Entities() {
		if entity == nil {
			continue
		}
		data := cloneMap(entity.Data)
		uuid := entity.UUID
		if uuid == nil {
			if rawUUID, ok := findKeyFold(data, "UUID"); ok {
				if parsed, ok := asUUID(rawUUID); ok {
					uuid = &parsed
				}
			}
		}
		deleteKeysFold(data, "id", "pos", "rotation", "motion", "uuid")

		if entity.ID != "" {
			data["id"] = entity.ID
		}
		data["Pos"] = []float64{entity.Pos[0], entity.Pos[1], entity.Pos[2]}
		data["Rotation"] = []float32{entity.Rotation[0], entity.Rotation[1]}
		data["Motion"] = []float64{entity.Motion[0], entity.Motion[1], entity.Motion[2]}
		if uuid != nil {
			data["UUID"] = *uuid
		}

		entities = append(entities, map[string]any{
			"pos": []float64{entity.Pos[0], entity.Pos[1], entity.Pos[2]},
			"blockPos": [3]int32{
				int32(math.Floor(entity.Pos[0])),
				int32(math.Floor(entity.Pos[1])),
				int32(math.Floor(entity.Pos[2])),
			},
			"nbt": sanitizeNBT(data),
		})
	}

	dataVersion := s.DataVersion()
	if dataVersion <= 0 {
		dataVersion = defaultDataVersion
	}
	root := map[string]any{
		"DataVersion": int32(dataVersion),
		"size":        [3]int32{int32(width), int32(height), int32(length)},
		"palette":     palette.entries,
		"blocks":      blocks,
		"entities":    entities,
	}

	gz := gzip.NewWriter(w)
	if err := nbt.NewEncoderWithEncoding(gz, nbt.BigEndian).Encode(root); err != nil {
		gz.Close()
		return fmt.Errorf("encode vanilla structure NBT: %w", err)
	}
	if err := gz.Close(); err != nil {
		return fmt.Errorf("compress vanilla structure NBT: %w", err)
	}
	return nil
}
