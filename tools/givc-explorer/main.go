// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package main

import (
	"context"
	"flag"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"unsafe"

	"github.com/bufbuild/protocompile"
	"github.com/bufbuild/protocompile/linker"
	"google.golang.org/protobuf/reflect/protoreflect"
)

const (
	reset     = "\033[0m"
	bold      = "\033[1m"
	dim       = "\033[2m"
	clearLine = "\033[2K\r"
	hideCur   = "\033[?25l"
	showCur   = "\033[?25h"
	altOn     = "\033[?1049h" // enter alternate screen
	altOff    = "\033[?1049l" // leave alternate screen
	clearScr  = "\033[2J\033[H"

	fgBlack  = "\033[30m"
	fgGreen  = "\033[32m"
	fgYellow = "\033[33m"
	fgBlue   = "\033[34m"
	fgCyan   = "\033[36m"
	fgWhite  = "\033[97m"
	fgGray   = "\033[90m"

	bgCyan  = "\033[46m"
	bgBlue  = "\033[44m"
	bgBlack = "\033[40m"
)

func color(c, s string) string { return c + s + reset }
func hiItem(s string) string   { return bgCyan + fgBlack + bold + " " + s + " " + reset }
func dimItem(s string) string  { return fgGray + "  " + s + reset }
func tag(s string) string      { return bold + fgCyan + "[" + s + "]" + reset }

type termios struct {
	Iflag, Oflag, Cflag, Lflag uint32
	Cc                         [20]uint8
	Ispeed, Ospeed             uint32
}

var origTermios termios

func setRaw() {
	syscall.Syscall(syscall.SYS_IOCTL, 0, syscall.TCGETS, uintptr(unsafe.Pointer(&origTermios)))
	raw := origTermios
	raw.Lflag &^= syscall.ECHO | syscall.ICANON | syscall.ISIG
	raw.Cc[syscall.VMIN] = 1
	raw.Cc[syscall.VTIME] = 0
	syscall.Syscall(syscall.SYS_IOCTL, 0, syscall.TCSETS, uintptr(unsafe.Pointer(&raw)))
}

func restoreTerminal() {
	syscall.Syscall(syscall.SYS_IOCTL, 0, syscall.TCSETS, uintptr(unsafe.Pointer(&origTermios)))
	fmt.Print(showCur, altOff)
}

func readKey() string {
	buf := make([]byte, 4)
	n, _ := os.Stdin.Read(buf)
	if n == 0 {
		return ""
	}
	switch {
	case buf[0] == 27 && n >= 3 && buf[1] == '[':
		switch buf[2] {
		case 'A':
			return "up"
		case 'B':
			return "down"
		case 'C':
			return "right"
		case 'D':
			return "left"
		}
	case buf[0] == 13 || buf[0] == 10:
		return "enter"
	case buf[0] == 27:
		return "esc"
	case buf[0] == 'q' || buf[0] == 'Q':
		return "q"
	case buf[0] == 'b' || buf[0] == 'B':
		return "b"
	}
	return string(buf[:n])
}

type winSize struct {
	Row, Col, Xpixel, Ypixel uint16
}

func termWidth() int {
	var ws winSize
	syscall.Syscall(syscall.SYS_IOCTL, 1, uintptr(syscall.TIOCGWINSZ), uintptr(unsafe.Pointer(&ws)))
	if ws.Col == 0 {
		return 80
	}
	return int(ws.Col)
}

func renderMenu(title, subtitle string, items []string, cursor int, hint string) {
	fmt.Print(clearScr)
	w := termWidth()
	line := strings.Repeat("─", w)

	fmt.Println(color(bold+fgCyan, " ◈ givc-acl-options"))
	fmt.Println(color(fgGray, line))
	fmt.Println()
	if title != "" {
		fmt.Printf("  %s\n\n", color(bold+fgWhite, title))
	}
	if subtitle != "" {
		fmt.Printf("  %s\n\n", color(fgGray, subtitle))
	}

	for i, item := range items {
		if i == cursor {
			fmt.Printf("  %s  %s\n", color(fgCyan, "▶"), hiItem(item))
		} else {
			fmt.Printf("  %s  %s\n", color(fgGray, " "), dimItem(item))
		}
	}

	fmt.Println()
	fmt.Println(color(fgGray, line))
	fmt.Printf("  %s  %s  %s\n",
		color(fgGray, "↑↓ navigate"),
		color(fgGray, "↵ select"),
		color(fgGray, hint),
	)
}

func pick(title, subtitle, hint string, items []string) int {
	cursor := 0
	for {
		renderMenu(title, subtitle, items, cursor, hint)
		key := readKey()
		switch key {
		case "up":
			if cursor > 0 {
				cursor--
			}
		case "down":
			if cursor < len(items)-1 {
				cursor++
			}
		case "enter", "right":
			return cursor
		case "esc", "b", "left":
			return -1 // back / cancel
		case "q":
			restoreTerminal()
			os.Exit(0)
		}
	}
}

func viewDetail(lines []string, title string) {
	for {
		fmt.Print(clearScr)
		w := termWidth()
		line := strings.Repeat("─", w)

		fmt.Println(color(bold+fgCyan, " ◈ proto-scanner"))
		fmt.Println(color(fgGray, line))
		fmt.Println()
		fmt.Printf("  %s\n\n", color(bold+fgWhite, title))

		for _, l := range lines {
			fmt.Println(l)
		}

		fmt.Println()
		fmt.Println(color(fgGray, line))
		fmt.Printf("  %s  %s\n",
			color(fgGray, "b/esc/← back"),
			color(fgGray, "q quit"),
		)

		key := readKey()
		switch key {
		case "b", "esc", "left", "q":
			return
		}
	}
}

func findProtos(root, module string) []string {
	var files []string
	targetDir := filepath.Join(root, module)
	filepath.Walk(targetDir, func(path string, info os.FileInfo, err error) error {
		if err == nil && !info.IsDir() && strings.HasSuffix(info.Name(), ".proto") {
			rel, _ := filepath.Rel(root, path)
			files = append(files, rel)
		}
		return nil
	})
	return files
}

func containsProto(path string) bool {
	found := false
	filepath.Walk(path, func(p string, info os.FileInfo, err error) error {
		if err == nil && !info.IsDir() && strings.HasSuffix(info.Name(), ".proto") {
			found = true
			return filepath.SkipDir
		}
		return nil
	})
	return found
}

func listModules(root string) []string {
	entries, err := os.ReadDir(root)
	if err != nil {
		log.Fatalf("Could not read directory: %v", err)
	}
	var mods []string
	for _, e := range entries {
		if e.IsDir() && !strings.HasPrefix(e.Name(), ".") {
			if containsProto(filepath.Join(root, e.Name())) {
				mods = append(mods, e.Name())
			}
		}
	}
	return mods
}

func compileModule(root, module string) []protoreflect.FileDescriptor {
	protoFiles := findProtos(root, module)
	if len(protoFiles) == 0 {
		return nil
	}
	compiler := protocompile.Compiler{
		Resolver:       &protocompile.SourceResolver{ImportPaths: []string{root}},
		SourceInfoMode: protocompile.SourceInfoStandard,
	}
	fds, err := compiler.Compile(context.Background(), protoFiles...)
	if err != nil {
		log.Fatalf("Compilation Error: %v", err)
	}
	var result []protoreflect.FileDescriptor
	for _, fd := range fds {
		result = append(result, fd.(linker.File))
	}
	return result
}

func getHelpText(loc protoreflect.SourceLocation) string {
	if strings.TrimSpace(loc.LeadingComments) != "" {
		return loc.LeadingComments
	}
	if strings.TrimSpace(loc.TrailingComments) != "" {
		return loc.TrailingComments
	}
	if len(loc.LeadingDetachedComments) > 0 {
		var parts []string
		for _, dc := range loc.LeadingDetachedComments {
			if t := strings.TrimSpace(dc); t != "" {
				parts = append(parts, t)
			}
		}
		return strings.Join(parts, " ")
	}
	return ""
}

func clean(c string) string {
	c = strings.TrimSpace(c)
	if c == "" {
		return color(fgCyan, "No documentation provided.")
	}
	lines := strings.Split(c, "\n")
	var parts []string
	for _, l := range lines {
		if t := strings.TrimSpace(l); t != "" {
			parts = append(parts, t)
		}
	}
	return strings.Join(parts, " ")
}

func rpcDetailLines(fd protoreflect.FileDescriptor, m protoreflect.MethodDescriptor) []string {
	var lines []string
	loc := fd.SourceLocations().ByDescriptor(m)

	lines = append(lines,
		fmt.Sprintf("  %s %s", tag("RPC"), color(bold+fgGreen, string(m.Name()))),
		fmt.Sprintf("  %s %s", color(fgGray, "Help    :"), color(fgBlue, clean(getHelpText(loc)))),
		"",
	)

	input := m.Input()
	inputFd := input.ParentFile()
	lines = append(lines,
		fmt.Sprintf("  %s %s", tag("Input"), color(bold+fgYellow, string(input.Name()))),
	)

	fields := input.Fields()
	for i := 0; i < fields.Len(); i++ {
		f := fields.Get(i)
		fLoc := inputFd.SourceLocations().ByDescriptor(f)
		kind := f.Kind().String()
		if f.IsList() {
			kind = "repeated " + kind
		}
		lines = append(lines,
			fmt.Sprintf("    %s %s  %s",
				color(fgCyan, "▸"),
				color(bold+fgWhite, string(f.Name())),
				color(fgGray, "("+kind+")"),
			),
			fmt.Sprintf("      %s %s",
				color(fgGray, "Help:"),
				color(fgBlue, clean(getHelpText(fLoc))),
			),
		)
	}

	return lines
}

// rpcMenu shows all RPCs in a module and lets the user drill into one.
func rpcMenu(root, module string) {
	fds := compileModule(root, module)
	if fds == nil {
		viewDetail([]string{color(fgYellow, "  No .proto files found in this module.")}, module)
		return
	}

	// Collect all (fd, method) pairs
	type entry struct {
		fd     protoreflect.FileDescriptor
		method protoreflect.MethodDescriptor
		label  string
	}
	var entries []entry
	for _, fd := range fds {
		for i := 0; i < fd.Services().Len(); i++ {
			svc := fd.Services().Get(i)
			svcLoc := fd.SourceLocations().ByDescriptor(svc)
			svcHelp := clean(getHelpText(svcLoc))
			_ = svcHelp
			methods := svc.Methods()
			for j := 0; j < methods.Len(); j++ {
				m := methods.Get(j)
				loc := fd.SourceLocations().ByDescriptor(m)
				help := clean(getHelpText(loc))
				label := string(m.Name())
				// append short help inline if it fits
				if help != color(fgGray+dim, "No documentation provided.") {
					short := help
					if len(short) > 50 {
						short = short[:47] + "..."
					}
					label = fmt.Sprintf("%-30s  %s", string(m.Name()), color(fgCyan, short))
				}
				entries = append(entries, entry{fd, m, label})
			}
		}
	}

	if len(entries) == 0 {
		viewDetail([]string{color(fgYellow, "  No RPC methods found.")}, module)
		return
	}

	labels := make([]string, len(entries))
	for i, e := range entries {
		labels[i] = e.label
	}

	for {
		idx := pick(
			"Module: "+module,
			"Select an RPC method to inspect",
			"b/esc back  •  q quit",
			labels,
		)
		if idx < 0 {
			return
		}
		e := entries[idx]
		lines := rpcDetailLines(e.fd, e.method)
		viewDetail(lines, string(e.method.Name()))
	}
}

func moduleMenu(root string) {
	modules := listModules(root)
	if len(modules) == 0 {
		fmt.Println("No modules with .proto files found in", root)
		return
	}

	items := append(modules, color(fgYellow, "Exit"))

	for {
		idx := pick(
			"Select a module",
			"Root: "+root,
			"q quit",
			items,
		)
		if idx < 0 || idx == len(modules) {
			// Exit chosen or ESC on root
			fmt.Print(clearScr)
			fmt.Println(color(fgGray, "Bye!"))
			return
		}
		rpcMenu(root, modules[idx])
	}
}

func main() {
	rootDir := flag.String("d", "", "Root directory of protos (e.g., ../api/)")
	flag.Parse()

	if *rootDir == "" {
		fmt.Fprintf(os.Stderr, "Usage: proto-scanner -d <proto-root>\n")
		os.Exit(1)
	}
	if _, err := os.Stat(*rootDir); os.IsNotExist(err) {
		fmt.Fprintf(os.Stderr, "Error: directory '%s' does not exist.\n", *rootDir)
		os.Exit(1)
	}

	// Enter alternate screen + raw mode
	fmt.Print(altOn, hideCur)
	setRaw()
	defer restoreTerminal()

	moduleMenu(*rootDir)
	restoreTerminal()
}
