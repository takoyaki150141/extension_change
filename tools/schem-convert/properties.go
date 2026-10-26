package main

import (
	"fmt"

	"github.com/oriumgames/schem/format"
)

// stringProperties presents block state property values as strings. Litematica
// and Axiom store them as strings, while the library's Sponge and MCEdit
// readers turn them into bool/int32. Without this, a Sponge input would produce
// a Litematica or Axiom file with NBT byte/int property tags.
type stringProperties struct {
	format.Schematic
	cache map[string]*format.BlockState
}

func stringifyProperties(s format.Schematic) format.Schematic {
	return &stringProperties{
		Schematic: s,
		cache:     make(map[string]*format.BlockState),
	}
}

func (s *stringProperties) Block(x, y, z int) *format.BlockState {
	block := s.Schematic.Block(x, y, z)
	if block == nil || len(block.Properties) == 0 {
		return block
	}

	key := blockStateKey(block.Name, block.Properties)
	if cached, ok := s.cache[key]; ok {
		return cached
	}

	props := make(map[string]any, len(block.Properties))
	for k, v := range block.Properties {
		props[k] = fmt.Sprint(v)
	}
	out := &format.BlockState{Name: block.Name, Properties: props}
	s.cache[key] = out
	return out
}
