package main

import (
	"maps"

	"github.com/oriumgames/schem/format"
)

// schematic is a sparse in-memory schematic. It is used for vanilla structure
// NBT, which is not supported by github.com/oriumgames/schem/format.
type schematic struct {
	width, height, length     int
	offsetX, offsetY, offsetZ int

	blocks        map[int]*format.BlockState
	blockEntities map[int]*format.BlockEntity
	biomes        map[int]string
	entities      []*format.Entity
	metadata      map[string]any
	dataVersion   int
}

var _ format.Schematic = (*schematic)(nil)

func newSchematic(width, height, length int) *schematic {
	return &schematic{
		width:         width,
		height:        height,
		length:        length,
		blocks:        make(map[int]*format.BlockState),
		blockEntities: make(map[int]*format.BlockEntity),
		biomes:        make(map[int]string),
		entities:      make([]*format.Entity, 0),
		metadata:      make(map[string]any),
	}
}

func (s *schematic) index(x, y, z int) int {
	return x + z*s.width + y*s.width*s.length
}

func (s *schematic) inBounds(x, y, z int) bool {
	return x >= 0 && x < s.width && y >= 0 && y < s.height && z >= 0 && z < s.length
}

func (s *schematic) Dimensions() (int, int, int) {
	return s.width, s.height, s.length
}

func (s *schematic) Offset() (int, int, int) {
	return s.offsetX, s.offsetY, s.offsetZ
}

func (s *schematic) SetOffset(x, y, z int) {
	s.offsetX, s.offsetY, s.offsetZ = x, y, z
}

func (s *schematic) Block(x, y, z int) *format.BlockState {
	if !s.inBounds(x, y, z) {
		return nil
	}
	return s.blocks[s.index(x, y, z)]
}

func (s *schematic) SetBlock(x, y, z int, block *format.BlockState) {
	if !s.inBounds(x, y, z) {
		return
	}
	idx := s.index(x, y, z)
	if block == nil {
		delete(s.blocks, idx)
	} else {
		s.blocks[idx] = block
	}
}

func (s *schematic) BlockEntity(x, y, z int) *format.BlockEntity {
	if !s.inBounds(x, y, z) {
		return nil
	}
	return s.blockEntities[s.index(x, y, z)]
}

func (s *schematic) SetBlockEntity(x, y, z int, be *format.BlockEntity) {
	if !s.inBounds(x, y, z) {
		return
	}
	idx := s.index(x, y, z)
	if be == nil {
		delete(s.blockEntities, idx)
	} else {
		be.X, be.Y, be.Z = x, y, z
		s.blockEntities[idx] = be
	}
}

func (s *schematic) Entities() []*format.Entity {
	out := make([]*format.Entity, len(s.entities))
	copy(out, s.entities)
	return out
}

func (s *schematic) AddEntity(entity *format.Entity) {
	s.entities = append(s.entities, entity)
}

func (s *schematic) RemoveEntity(entity *format.Entity) {
	for i, e := range s.entities {
		if e == entity {
			s.entities = append(s.entities[:i], s.entities[i+1:]...)
			return
		}
	}
}

func (s *schematic) Biome(x, y, z int) string {
	if x < 0 || x >= s.width || z < 0 || z >= s.length {
		return ""
	}
	if y >= 0 && y < s.height {
		if biome, ok := s.biomes[s.index(x, y, z)]; ok {
			return biome
		}
	}
	return s.biomes[x+z*s.width]
}

func (s *schematic) SetBiome(x, y, z int, biome string) {
	if x < 0 || x >= s.width || z < 0 || z >= s.length {
		return
	}
	var idx int
	if y >= 0 && y < s.height {
		idx = s.index(x, y, z)
	} else {
		idx = x + z*s.width
	}
	if biome == "" {
		delete(s.biomes, idx)
	} else {
		s.biomes[idx] = biome
	}
}

func (s *schematic) Metadata() map[string]any {
	out := make(map[string]any, len(s.metadata))
	maps.Copy(out, s.metadata)
	return out
}

func (s *schematic) SetMetadata(key string, value any) {
	s.metadata[key] = value
}

func (s *schematic) Format() string {
	return "nbt"
}

func (s *schematic) DataVersion() int {
	return s.dataVersion
}

func (s *schematic) SetDataVersion(version int) {
	s.dataVersion = version
}

func (s *schematic) Version() string {
	return ""
}
