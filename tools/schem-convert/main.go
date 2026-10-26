package main

import (
	"bytes"
	"encoding/binary"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/oriumgames/schem/format"
)

const axiomMagic uint32 = 0x0AE5BB36

type outputTarget struct {
	libraryFormat string
	extension     string
}

var outputTargets = map[string]outputTarget{
	"litematica": {"litematica_v7", ".litematic"},
	"schem":      {"sponge_v3", ".schem"},
	"bp":         {"axiom", ".bp"},
	"schematic":  {"mcedit", ".schematic"},
	"nbt":        {"", ".nbt"},
}

func main() {
	os.Exit(run(os.Args[1:], os.Stderr))
}

func run(args []string, stderr io.Writer) int {
	fs := flag.NewFlagSet("schem-convert", flag.ContinueOnError)
	fs.SetOutput(stderr)

	var inputPath, outputPath, formatName string
	fs.StringVar(&inputPath, "in", "", "input schematic path")
	fs.StringVar(&outputPath, "out", "", "output schematic path")
	fs.StringVar(&formatName, "format", "", "output format: litematica, schem, bp, schematic, nbt")
	fs.Usage = func() {
		fmt.Fprintln(stderr, "Usage: schem-convert -in <input-path> -out <output-path> -format <litematica|schem|bp|schematic|nbt>")
	}

	if err := fs.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return 0
		}
		return 2
	}
	if fs.NArg() > 0 {
		fmt.Fprintf(stderr, "schem-convert: unexpected argument %q\n", fs.Arg(0))
		fs.Usage()
		return 2
	}
	if inputPath == "" {
		fmt.Fprintln(stderr, "schem-convert: -in is required")
		fs.Usage()
		return 2
	}
	if outputPath == "" {
		fmt.Fprintln(stderr, "schem-convert: -out is required")
		fs.Usage()
		return 2
	}
	if formatName == "" {
		fmt.Fprintln(stderr, "schem-convert: -format is required")
		fs.Usage()
		return 2
	}

	target, ok := outputTargets[strings.ToLower(formatName)]
	if !ok {
		fmt.Fprintf(stderr, "schem-convert: unsupported output format %q (supported: litematica, schem, bp, schematic, nbt)\n", formatName)
		return 2
	}

	schem, err := readSchematicFile(inputPath)
	if err != nil {
		fmt.Fprintf(stderr, "schem-convert: %v\n", err)
		return 1
	}

	if err := writeSchematicFile(outputPath, target, schem); err != nil {
		fmt.Fprintf(stderr, "schem-convert: %v\n", err)
		return 1
	}

	return 0
}

func readSchematicFile(path string) (format.Schematic, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read input %q: %w", path, err)
	}
	if len(data) < 4 {
		return nil, fmt.Errorf("read input %q: file is too small (%d bytes)", path, len(data))
	}

	if binary.BigEndian.Uint32(data[:4]) == axiomMagic {
		schem, err := format.Read(bytes.NewReader(data))
		if err != nil {
			return nil, fmt.Errorf("read Axiom blueprint %q: %w", path, err)
		}
		return patchAxiomEntities(schem, data), nil
	}

	// Vanilla structure blocks are gzip-compressed (or, occasionally, raw or
	// zlib-compressed) NBT. The library does not detect them.
	if root, ok := tryReadRootNBT(data); ok {
		if isVanillaStructureRoot(root) {
			schem, err := parseVanillaStructure(root)
			if err != nil {
				return nil, fmt.Errorf("read vanilla structure %q: %w", path, err)
			}
			return schem, nil
		}
		// Sponge v3 wraps its payload in a "Schematic" compound, which the
		// library's Detect does not look into.
		if formatID := detectNestedSpongeV3(root); formatID != "" {
			input := data
			if formatID == "sponge_v3" {
				if compatible, ok := spongeV3ForLibrary(data); ok {
					input = compatible
				}
			}
			schem, err := format.ReadFormat(bytes.NewReader(input), formatID)
			if err != nil {
				return nil, fmt.Errorf("read input %q as %s: %w", path, formatID, err)
			}
			return schem, nil
		}
	}

	formatID, err := format.Detect(data)
	if err != nil {
		return nil, fmt.Errorf("detect input format %q: %w", path, err)
	}
	schem, err := format.ReadFormat(bytes.NewReader(data), formatID)
	if err != nil {
		return nil, fmt.Errorf("read input %q as %s: %w", path, formatID, err)
	}
	return schem, nil
}

func writeSchematicFile(path string, target outputTarget, schem format.Schematic) error {
	write := func(w io.Writer) error {
		switch target.libraryFormat {
		case "":
			return writeVanillaNBT(w, schem)
		case "axiom":
			return writeAxiom(w, stringifyProperties(schem))
		case "sponge_v3":
			return writeSpongeV3(w, schem)
		case "litematica_v7":
			return format.WriteFormat(w, target.libraryFormat, stringifyProperties(schem))
		default:
			return format.WriteFormat(w, target.libraryFormat, schem)
		}
	}

	f, err := os.Create(path)
	if err != nil {
		return fmt.Errorf("create output %q: %w", path, err)
	}
	if err := write(f); err != nil {
		f.Close()
		os.Remove(path)
		return fmt.Errorf("write output %q: %w", path, err)
	}
	if err := f.Close(); err != nil {
		os.Remove(path)
		return fmt.Errorf("close output %q: %w", path, err)
	}
	return nil
}
