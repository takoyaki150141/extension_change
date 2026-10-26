package main

import (
	"bytes"
	"encoding/binary"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"

	"github.com/oriumgames/schem/format"
)

func newTestSchematic() format.Schematic {
	s := newSchematic(3, 3, 3)
	s.SetDataVersion(3465)

	stone := &format.BlockState{Name: "minecraft:stone"}
	for y := 0; y < 3; y++ {
		for z := 0; z < 3; z++ {
			for x := 0; x < 3; x++ {
				s.SetBlock(x, y, z, stone.Clone())
			}
		}
	}

	s.SetBlock(1, 1, 1, &format.BlockState{
		Name: "minecraft:chest",
		Properties: map[string]any{
			"facing": "north",
			"type":   "single",
		},
	})
	s.SetBlockEntity(1, 1, 1, &format.BlockEntity{
		ID: "minecraft:chest",
		Data: map[string]any{
			"CustomName": "{\"text\":\"Test Chest\"}",
			"Lock":       "",
		},
	})

	s.SetBlock(0, 2, 0, &format.BlockState{
		Name: "minecraft:oak_stairs",
		Properties: map[string]any{
			"facing": "east",
			"half":   "bottom",
			"shape":  "outer_right",
		},
	})

	s.AddEntity(&format.Entity{
		ID:       "minecraft:armor_stand",
		Pos:      [3]float64{1.5, 1.0, 1.5},
		Rotation: [2]float32{90, 0},
		Data: map[string]any{
			"CustomName": "{\"text\":\"Test Stand\"}",
		},
	})

	return s
}

func TestAllOutputFormatsRoundTrip(t *testing.T) {
	dir := t.TempDir()
	src := newTestSchematic()

	for _, name := range []string{"litematica", "schem", "bp", "schematic", "nbt"} {
		t.Run(name, func(t *testing.T) {
			target := outputTargets[name]
			out := filepath.Join(dir, "out-"+name+target.extension)
			if err := writeSchematicFile(out, target, src); err != nil {
				t.Fatalf("write: %v", err)
			}
			if name == "bp" {
				assertAxiomHeader(t, out)
			}
			if name == "schem" {
				assertSpongeV3BlockEntityPosIsIntArray(t, out)
			}

			readBack, err := readSchematicFile(out)
			if err != nil {
				t.Fatalf("read back: %v", err)
			}
			assertTestStructure(t, name, readBack)
		})
	}
}

func TestLitematicaAndAxiomStringifyProperties(t *testing.T) {
	dir := t.TempDir()

	src := newSchematic(1, 1, 1)
	src.SetDataVersion(3465)
	src.SetBlock(0, 0, 0, &format.BlockState{
		Name: "minecraft:oak_stairs",
		Properties: map[string]any{
			"facing":      "east",
			"half":        "bottom",
			"shape":       "straight",
			"waterlogged": false,
		},
	})

	for _, name := range []string{"litematica", "bp"} {
		t.Run(name, func(t *testing.T) {
			out := filepath.Join(dir, "props-"+name+outputTargets[name].extension)
			if err := writeSchematicFile(out, outputTargets[name], src); err != nil {
				t.Fatalf("write: %v", err)
			}
			readBack, err := readSchematicFile(out)
			if err != nil {
				t.Fatalf("read: %v", err)
			}
			block := readBack.Block(0, 0, 0)
			if block == nil {
				t.Fatalf("missing block")
			}
			value, ok := block.Properties["waterlogged"].(string)
			if !ok || value != "false" {
				t.Fatalf("waterlogged = %#v (%T), want string \"false\"", block.Properties["waterlogged"], block.Properties["waterlogged"])
			}
		})
	}
}

func TestCLIConvertsBetweenFormats(t *testing.T) {
	dir := t.TempDir()
	src := newTestSchematic()

	input := filepath.Join(dir, "input.nbt")
	if err := writeSchematicFile(input, outputTargets["nbt"], src); err != nil {
		t.Fatalf("write input: %v", err)
	}

	produced := make(map[string]string)
	for _, name := range []string{"litematica", "schem", "bp", "schematic", "nbt"} {
		out := filepath.Join(dir, "cli-"+name+outputTargets[name].extension)
		var stderr bytes.Buffer
		if code := run([]string{"-in", input, "-out", out, "-format", name}, &stderr); code != 0 {
			t.Fatalf("%s: exit %d, stderr: %s", name, code, stderr.String())
		}
		readBack, err := readSchematicFile(out)
		if err != nil {
			t.Fatalf("%s: read back: %v", name, err)
		}
		assertTestStructure(t, name, readBack)
		produced[name] = out
	}

	// Convert the Litematica output through every remaining format.
	current := produced["litematica"]
	for _, name := range []string{"schem", "bp", "schematic", "nbt"} {
		out := filepath.Join(dir, "chain-"+name+outputTargets[name].extension)
		var stderr bytes.Buffer
		if code := run([]string{"-in", current, "-out", out, "-format", name}, &stderr); code != 0 {
			t.Fatalf("chain to %s: exit %d, stderr: %s", name, code, stderr.String())
		}
		current = out
	}

	final, err := readSchematicFile(current)
	if err != nil {
		t.Fatalf("read chain result: %v", err)
	}
	assertTestStructure(t, "chain", final)
}

func TestVanillaNBTReadsRawAndCompressed(t *testing.T) {
	dir := t.TempDir()
	src := newTestSchematic()

	compressed := filepath.Join(dir, "structure.nbt")
	if err := writeSchematicFile(compressed, outputTargets["nbt"], src); err != nil {
		t.Fatalf("write: %v", err)
	}

	data, err := os.ReadFile(compressed)
	if err != nil {
		t.Fatal(err)
	}
	if len(data) < 2 || data[0] != 0x1F || data[1] != 0x8B {
		t.Fatalf("vanilla output is not gzip-compressed")
	}

	raw, ok := gunzip(data)
	if !ok {
		t.Fatalf("gunzip failed")
	}
	rawPath := filepath.Join(dir, "structure-raw.nbt")
	if err := os.WriteFile(rawPath, raw, 0o644); err != nil {
		t.Fatal(err)
	}

	readBack, err := readSchematicFile(rawPath)
	if err != nil {
		t.Fatalf("read raw NBT: %v", err)
	}
	assertTestStructure(t, "raw-nbt", readBack)
}

func TestCLIErrors(t *testing.T) {
	dir := t.TempDir()

	var stderr bytes.Buffer
	if code := run(nil, &stderr); code == 0 {
		t.Fatalf("expected non-zero exit for missing arguments")
	}
	if stderr.Len() == 0 {
		t.Fatalf("expected a message on stderr")
	}

	stderr.Reset()
	missing := filepath.Join(dir, "does-not-exist.nbt")
	if code := run([]string{"-in", missing, "-out", filepath.Join(dir, "out.schem"), "-format", "schem"}, &stderr); code == 0 {
		t.Fatalf("expected non-zero exit for missing input")
	}
	if stderr.Len() == 0 {
		t.Fatalf("expected a message on stderr")
	}

	stderr.Reset()
	if code := run([]string{"-in", missing, "-out", filepath.Join(dir, "out.bin"), "-format", "unknown"}, &stderr); code == 0 {
		t.Fatalf("expected non-zero exit for unknown format")
	}
	if stderr.Len() == 0 {
		t.Fatalf("expected a message on stderr")
	}
}

func TestExecutableEndToEnd(t *testing.T) {
	exeName := "schem-convert"
	if runtime.GOOS == "windows" {
		exeName += ".exe"
	}
	exe := filepath.Join(t.TempDir(), exeName)

	var buildStderr bytes.Buffer
	build := exec.Command("go", "build", "-trimpath", "-ldflags", "-s -w", "-o", exe, ".")
	build.Stderr = &buildStderr
	if err := build.Run(); err != nil {
		t.Fatalf("go build: %v\n%s", err, buildStderr.String())
	}

	dir := filepath.Join(t.TempDir(), "変換 テスト")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	input := filepath.Join(dir, "入力 structure.nbt")
	if err := writeSchematicFile(input, outputTargets["nbt"], newTestSchematic()); err != nil {
		t.Fatalf("write fixture: %v", err)
	}

	for _, name := range []string{"litematica", "schem", "bp", "schematic", "nbt"} {
		out := filepath.Join(dir, "出力 "+name+outputTargets[name].extension)
		var stderr bytes.Buffer
		cmd := exec.Command(exe, "-in", input, "-out", out, "-format", name)
		cmd.Stderr = &stderr
		if err := cmd.Run(); err != nil {
			t.Fatalf("%s: %v, stderr: %s", name, err, stderr.String())
		}
		if name == "bp" {
			assertAxiomHeader(t, out)
		}
		readBack, err := readSchematicFile(out)
		if err != nil {
			t.Fatalf("%s: read back: %v", name, err)
		}
		assertTestStructure(t, "exe-"+name, readBack)
	}

	var stderr bytes.Buffer
	cmd := exec.Command(exe, "-in", filepath.Join(dir, "存在しない.nbt"), "-out", filepath.Join(dir, "out.nbt"), "-format", "nbt")
	cmd.Stderr = &stderr
	if err := cmd.Run(); err == nil {
		t.Fatalf("expected a non-zero exit for a missing input")
	}
	if stderr.Len() == 0 {
		t.Fatalf("expected a message on stderr")
	}
}

func assertAxiomHeader(t *testing.T, path string) {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if len(data) < 4 {
		t.Fatalf("Axiom file is too short")
	}
	want := []byte{0x0A, 0xE5, 0xBB, 0x36}
	if !bytes.Equal(data[:4], want) {
		t.Fatalf("Axiom magic = % X, want % X", data[:4], want)
	}
	if got := binary.BigEndian.Uint32(data[:4]); got != axiomMagic {
		t.Fatalf("Axiom magic = %#x, want %#x", got, axiomMagic)
	}
	formatID, err := format.Detect(data)
	if err != nil {
		t.Fatalf("Detect: %v", err)
	}
	if formatID != "axiom" {
		t.Fatalf("Detect = %q, want axiom", formatID)
	}
}

func assertSpongeV3BlockEntityPosIsIntArray(t *testing.T, path string) {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	root, ok := tryReadRootNBT(data)
	if !ok {
		t.Fatalf("could not decode Sponge v3 output")
	}
	schematic, ok := asMap(root["Schematic"])
	if !ok {
		t.Fatalf("missing Schematic root")
	}
	blocks, ok := asMap(schematic["Blocks"])
	if !ok {
		t.Fatalf("missing Blocks")
	}
	entries, ok := asAnySlice(blocks["BlockEntities"])
	if !ok || len(entries) == 0 {
		t.Fatalf("missing BlockEntities")
	}
	be, ok := asMap(entries[0])
	if !ok {
		t.Fatalf("BlockEntities[0] is not a compound")
	}
	if _, ok := be["Pos"].([3]int32); !ok {
		t.Fatalf("BlockEntities[0].Pos is %T, want [3]int32 (TAG_IntArray)", be["Pos"])
	}
	if id, _ := asString(be["Id"]); id != "minecraft:chest" {
		t.Fatalf("BlockEntities[0].Id = %q, want minecraft:chest", be["Id"])
	}
	beData, ok := asMap(be["Data"])
	if !ok {
		t.Fatalf("BlockEntities[0].Data is missing")
	}
	if got, _ := asString(beData["CustomName"]); got != "{\"text\":\"Test Chest\"}" {
		t.Fatalf("BlockEntities[0].Data.CustomName = %q", beData["CustomName"])
	}

	entities, ok := asAnySlice(schematic["Entities"])
	if !ok || len(entities) == 0 {
		t.Fatalf("missing Entities")
	}
	entity, ok := asMap(entities[0])
	if !ok {
		t.Fatalf("Entities[0] is not a compound")
	}
	if id, _ := asString(entity["Id"]); id != "minecraft:armor_stand" {
		t.Fatalf("Entities[0].Id = %q, want minecraft:armor_stand", entity["Id"])
	}
	if _, ok := asFloat3(entity["Pos"]); !ok {
		t.Fatalf("Entities[0].Pos is %T, want a list of doubles", entity["Pos"])
	}
	entityData, ok := asMap(entity["Data"])
	if !ok {
		t.Fatalf("Entities[0].Data is missing")
	}
	if _, ok := entityData["Rotation"]; !ok {
		t.Fatalf("Entities[0].Data.Rotation is missing")
	}
}

func assertTestStructure(t *testing.T, label string, s format.Schematic) {
	t.Helper()

	width, height, length := s.Dimensions()
	if width != 3 || height != 3 || length != 3 {
		t.Fatalf("%s: dimensions = %dx%dx%d, want 3x3x3", label, width, height, length)
	}

	stoneCount := 0
	for y := 0; y < height; y++ {
		for z := 0; z < length; z++ {
			for x := 0; x < width; x++ {
				if block := s.Block(x, y, z); block != nil && block.Name == "minecraft:stone" {
					stoneCount++
				}
			}
		}
	}
	if stoneCount != 25 {
		t.Fatalf("%s: stone count = %d, want 25", label, stoneCount)
	}

	chest := s.Block(1, 1, 1)
	if chest == nil || chest.Name != "minecraft:chest" {
		t.Fatalf("%s: block at 1,1,1 = %v, want minecraft:chest", label, chest)
	}
	if got, _ := chest.Properties["facing"].(string); got != "north" {
		t.Fatalf("%s: chest facing = %q, want north", label, chest.Properties["facing"])
	}

	stairs := s.Block(0, 2, 0)
	if stairs == nil || stairs.Name != "minecraft:oak_stairs" {
		t.Fatalf("%s: block at 0,2,0 = %v, want minecraft:oak_stairs", label, stairs)
	}
	for key, want := range map[string]string{"facing": "east", "half": "bottom", "shape": "outer_right"} {
		if got, _ := stairs.Properties[key].(string); got != want {
			t.Fatalf("%s: stairs %s = %q, want %q", label, key, stairs.Properties[key], want)
		}
	}

	be := s.BlockEntity(1, 1, 1)
	if be == nil {
		t.Fatalf("%s: missing block entity at 1,1,1", label)
	}
	if be.ID != "minecraft:chest" {
		t.Fatalf("%s: block entity ID = %q, want minecraft:chest", label, be.ID)
	}
	if got, _ := be.Data["CustomName"].(string); got != "{\"text\":\"Test Chest\"}" {
		t.Fatalf("%s: block entity CustomName = %q", label, be.Data["CustomName"])
	}

	entities := s.Entities()
	if len(entities) != 1 {
		t.Fatalf("%s: entity count = %d, want 1", label, len(entities))
	}
	entity := entities[0]
	if entity.ID != "minecraft:armor_stand" {
		t.Fatalf("%s: entity ID = %q, want minecraft:armor_stand", label, entity.ID)
	}
	if math.Abs(entity.Pos[0]-1.5) > 1e-6 || math.Abs(entity.Pos[1]-1.0) > 1e-6 || math.Abs(entity.Pos[2]-1.5) > 1e-6 {
		t.Fatalf("%s: entity Pos = %v, want [1.5 1 1.5]", label, entity.Pos)
	}
	if math.Abs(float64(entity.Rotation[0])-90) > 1e-6 {
		t.Fatalf("%s: entity yaw = %v, want 90", label, entity.Rotation[0])
	}
	if got, _ := entity.Data["CustomName"].(string); got != "{\"text\":\"Test Stand\"}" {
		t.Fatalf("%s: entity CustomName = %q", label, entity.Data["CustomName"])
	}
}
