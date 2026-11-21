================================================================
 SECTOR64 — USER MANUAL
================================================================

Sector64 is a native tool for browsing, extracting from, and
editing Commodore disk images and archives — a workbench for the
C64 world, built on a dependency-free Rust core.

This manual describes every feature of the graphical browser
(`sector64-gui`) and how to use it.

----------------------------------------------------------------
 CONTENTS
----------------------------------------------------------------

  1. Getting started
  2. Supported formats
  3. The window at a glance
  4. Opening a file
  5. Browsing the listing
       - Columns and type badges
       - Sorting
       - Filtering
       - Selecting files
       - Keyboard navigation
  6. The preview pane (HEX / TEXT / BASIC / DIS / GFX / PIC)
  7. Extracting files
  8. Editing a disk image
       - Creating a new disk
       - Adding files
       - Renaming
       - Deleting (scratching)
       - Saving
       - Undo and redo
       - The unsaved-changes guard
  9. Building an archive
 10. Validating a disk
 11. The disk map
 12. Copying between disks
 13. Running in VICE
 14. Status line and toast messages
 15. Keyboard shortcuts
 16. Settings that persist
 17. Tips and troubleshooting


----------------------------------------------------------------
 1. GETTING STARTED
----------------------------------------------------------------

Launch the browser from a terminal:

    sector64-gui

You can also open a file straight away by passing its path:

    sector64-gui disk.d64
    sector64-gui "MY ARCHIVE.ARC"

The window remembers its size, your chosen output folder and
your recent files between runs, so it comes back the way you
left it.


----------------------------------------------------------------
 2. SUPPORTED FORMATS
----------------------------------------------------------------

Disk images (read):
    .d64  .d71  .d81  .g64  .d2m
    Zip-Code sets  (1!NAME … 4!NAME — open any one of the four
                    parts; all four must be in the same folder)

Disk images (read + write / edit):
    .d64  .d71  .d81

Archives (read + extract):
    .arc  .sda   (ARC / self-dissolving ARC)
    .lha  .lzh  .sfx   (LHA and self-extracting LHA)
    .lbr  (Library)
    .t64  (tape image)
    .lnx  (Lynx)
    .p00  (PC64 single-file container)
    .wr3  (Wraptor 3)

Archives you can create and save:
    .t64  .lnx  .lbr  .p00

When you open an archive, each member is decompressed on the
fly. For compressed archives (ARC/SDA, LHA) the PACKED column
shows each member's stored size and method next to its full SIZE. A member that uses a decoder Sector64 does not implement is
listed with the reason in place of its bytes, instead of failing
the whole archive.


----------------------------------------------------------------
 3. THE WINDOW AT A GLANCE
----------------------------------------------------------------

Default layout (the preview pane is hidden until you turn it on):

  +----------------------------------------------------------+
  | [=] SECTOR64  File | Disk | Preview | Help                 |  <- menu bar
  +----------------------------------------------------------+
  | Disk 0 "MY DISK" 1A                          664 free    |  <- header row
  | Filter: [ type to filter by name                       ] |  <- filter box
  | TYPE  NAME              BLK          SIZE                |  <- column heads
  | PRG   HELLO              3 BLK       689 BYTES           |
  | SEQ   README             2 BLK       410 BYTES           |  <- listing
  |  ...                                                     |
  +----------------------------------------------------------+
  | Open:   /path/to/the/file/you/opened.d64                 |  <- opened path
  | Output: /path/to/your/extract/folder                     |  <- output folder
  | 24 item(s) loaded                                        |  <- status line
  +----------------------------------------------------------+

With the preview pane turned on (menu bar > Preview), it opens on
the right and the listing makes room for it:

  +----------------------------------------------------------+
  | [=] SECTOR64  File | Disk |[Preview]| Help                 |  <- Preview lit
  +----------------------------------------------------------+
  | Disk 0 "MY DISK" 1A   664 free |  PREVIEW — HELLO        |  <- header | preview
  | Filter: [ filter by name     ] |           HEX | TEXT    |     head + toggle
  | TYPE  NAME       BLK     SIZE  |  0000: 01 08 ...  ..    |
  | PRG   HELLO       3 BLK  689 B |  0010: 0B 08 ...  ..    |  <- listing | preview
  | SEQ   README      2 BLK  410 B |  ...                    |
  +----------------------------------------------------------+
  | Open:   /path/to/the/file/you/opened.d64                 |
  | Output: /path/to/your/extract/folder                     |
  | 24 item(s) loaded                                        |
  +----------------------------------------------------------+

- Menu bar (top):
    File     New, Open, Open Recent, Add file, Save, Save As,
             Extract ▸, Copy..., Run in VICE, Undo, Redo,
             Rename and Delete.
    Disk     Validate Disk and Disk Map.
    Preview  shows or hides the preview pane.
    Help     this manual.
- Header row: the disk/archive title and, for disks, blocks
  free. Shown as the first line of the listing.
- Filter box: live name filter.
- Listing: the files, in columns, with colour-coded badges. It
  uses the full width until the preview pane is shown.
- Preview pane (right): hidden by default; turn it on with the
  Preview menu item. Shows the selected file's bytes as hex or
  text. It sizes itself to fit (see 6).
- Bottom bar: the opened file path, the output folder, and the
  status line.
- Toasts: brief messages pop in at the upper-right.


----------------------------------------------------------------
 4. OPENING A FILE
----------------------------------------------------------------

Four ways:

  - Menu:  File > Open…  (or press Ctrl+O), then pick a file.
  - Recent files:  File > Open Recent, or click a name under
    RECENT FILES on the start screen.
  - Launch argument:  sector64-gui <path>
  - Drag a disk image or archive onto the window.

Open Recent lists the last 10 files you opened or saved, newest
first. Hover over a name to see its full path; when two files
share a name, the folder each is in is shown after it. A file
that has been moved or deleted is dropped from the list when you
pick it. "Clear list", at the bottom of the menu, empties it.

Dragging opens the file only when nothing editable is open (the
start screen, or a read-only file such as an ARC or G64). With an
editable disk or a new archive open, dropped files are added to
it instead (see 8). If you drop several files, the first is
opened and the rest are ignored.

On Linux, drag-and-drop works only when Sector64 runs on X11; see
"Drag-and-drop on Linux (Wayland)" in section 17.

When a file opens, the first entry is selected automatically, so
if you turn the preview pane on it has something to show at once.
The folder the file came from is suggested as the output folder
if you have not set one yet.


----------------------------------------------------------------
 5. BROWSING THE LISTING
----------------------------------------------------------------

COLUMNS AND TYPE BADGES

  TYPE   A colour-coded badge for the file type:
             PRG  program        (green)
             SEQ  sequential/text (cyan)
             USR  user            (purple)
             REL  relative        (yellow)
             DEL  deleted/scratched (grey)
  NAME   The Commodore file name (up to 16 characters).
  BLK    Size in 254-byte disk blocks (disk images only).
  SIZE   Size in bytes.

Columns are padded to line up, so sizes and units sit in neat
columns however long the names are.

SORTING

  Click a column header to sort by it:
      TYPE, NAME, BLK, or SIZE.
  Click the same header again to reverse the order, and a third
  time to return to the disk's own directory order. The active
  column shows a ▲ (ascending) or ▼ (descending) marker.

  Sorting is only a view — it never changes the file on disk.
  Selection, marks, and keyboard navigation all follow what is
  shown.

FILTERING

  Type in the Filter box to show only files whose name contains
  what you typed (case-insensitive). A (matched/total) count and
  an X button appear while a filter is active; click X to clear
  it. The filter clears automatically when you open a new file.

SELECTING FILES

  - Click a row to select it.
  - Ctrl+click toggles extra rows into a multi-selection.
  - Shift+click selects a range from the active row.
  Multi-selected rows are all highlighted, and Extract Selected
  will write every one of them. Right-clicking a row that is
  already part of a multi-selection keeps the selection intact.

KEYBOARD NAVIGATION

  Up / Down arrows move the selection through the list in the
  order currently displayed (sorted and/or filtered).


----------------------------------------------------------------
 6. THE PREVIEW PANE (HEX / TEXT / BASIC / DIS / GFX / PIC)
----------------------------------------------------------------

The preview pane is hidden by default. Click Preview in the menu
bar to show it; the item lights up while the pane is open, and
clicking it again hides the pane and returns the listing to the
full window width.

When shown, the pane on the right shows the bytes of the selected
file. Use the toggle in its header to switch views:

  HEX    An offset + hex + ASCII dump:
             0000: 01 08 0B 08 ...   ........
         Good for inspecting binaries and headers.

  TEXT   The bytes rendered as PETSCII text. Carriage returns
         become line breaks, printable characters show through,
         and non-text bytes show as a dot. Good for reading SEQ
         text files, DOCs, and strings embedded in programs.

  BASIC  A BASIC program listed the way LIST shows it on a C64:
             10 PRINT "{CLR}{5 DOWN}HELLO"
             20 GOTO 10
         Keywords are spelled out, line numbers are highlighted,
         and control codes inside quotes are shown by name, as
         in printed magazine listings ({CLR}, {RVS ON}, {RED},
         {5 DOWN} for five in a row). π shows as π. Codes with no
         usual name show as {$xx}.
         Three dialects are understood. The row under the
         buttons picks one:
           AUTO    guess (the default): BASIC 7.0 for programs
                   saved on a C128, Simons' BASIC when its
                   keywords are used, otherwise V2. A guess
                   other than V2 is noted on the first line,
                   e.g. "(listed as BASIC 7.0)".
           V2      Commodore BASIC V2 (C64, VIC-20).
           7.0     the C128's BASIC 7.0 (DO, LOOP, GRAPHIC...).
           SIMONS  Simons' BASIC for the C64 (HIRES, PROC...).
         Pick one by hand when the guess is wrong, e.g. a
         BASIC 7.0 program saved in C64 mode. Keywords a
         dialect doesn't have show as {$xx} (or {$xxxx} for
         two-byte tokens).
         A program that ends with a SYS line and machine code
         lists the BASIC part, then says how many bytes follow.
         A file that isn't a BASIC program says so; use HEX or
         TEXT for it.

  DIS    The bytes disassembled as 6502 machine code, the way a
         machine-code monitor lists it:
             C000  A9 00     LDA #$00
             C002  8D 20 D0  STA $D020
         Each line is the address, the instruction's bytes, and
         the instruction. Where the listing starts is noted on
         the first line:
           - a program with a BASIC line such as SYS 2061
             starts at the address the SYS jumps to, so a
             machine-code program with a BASIC starter lists
             its code, not the BASIC line;
           - any other PRG starts at the address it loads to;
           - a file with no load address (SEQ, USR...) starts
             at $0000.
         Undocumented opcodes (LAX, SAX, DCP, JAM...) are marked
         with * and dimmed. Real programs do use a few, but a
         run of them usually means the bytes there are data,
         such as graphics or text, rather than code.
         The first 8192 instructions are listed.

  GFX    The bytes drawn as graphics, the way the C64's video
         chip would show them. The row under the buttons picks
         what to draw:
           SPRITES  24x21 sprites, 64 bytes each.
           CHARS    8x8 characters, 8 bytes each, 16 to a row
                    like a character set.
           HIRES    one bit per pixel: white on black.
           MULTI    multicolour: each pair of bits is one
                    double-wide pixel, in black, white and the
                    two shared colours (light red, yellow).
         A PRG's graphics are lined up with the memory it loads
         to, as the C64 sees them: sprites start on a 64-byte
         boundary and characters on an 8-byte one, so a few
         bytes may be skipped at the start. Other files start
         at their first byte. The line above the picture says
         how many there are and where they start.
         Hover over a sprite or character to see its number and
         file offset; for a PRG also the address it loads to
         and the sprite pointer (or screen code) that shows it.
         Very large files show as many as the screen can hold
         and say so.

  PIC    A picture saved by a C64 paint program, in its own
         colours, scaled to fit the pane:
           Koala Painter        multicolour, loads at $6000
           Advanced Art Studio  multicolour, loads at $2000
           Art Studio           hires, loads at $2000
           Doodle!              hires, loads at $5C00
         The format is worked out from the file's load address
         and size (format: AUTO). If a picture isn't recognised,
         choose its format from the menu above to show it as
         that anyway. The line above the picture names the
         format; formats that save a border colour are framed
         in it. Hover over the picture to see a pixel's
         position, its 8x8 cell, and its colour.

The pane sizes itself: wide enough for a whole hex line, or for
40 columns of text, BASIC, disassembly, graphics or pictures (a C64 screen's width), whichever
mode is on. Long BASIC lines wrap at any character, as they do on
a C64 screen.
If the window is too narrow for that and the listing, the pane
gives way first, down to about 20 characters; hex lines then
scroll sideways instead of wrapping. If the window is narrower
still, the listing shortens long names, then hides the BLK /
PACKED column, then the SIZE column, so text never overlaps.
Widen the window to bring them back. Files that cannot be decoded (an unsupported archive member) show the
reason here instead of bytes.


----------------------------------------------------------------
 7. EXTRACTING FILES
----------------------------------------------------------------

First set where extracted files should go: click the Output:
button at the bottom and pick a folder. (It is pre-filled with
the folder your file came from, and it is remembered between
runs.)

Then extract:

  - One file:   select it and choose File > Extract ▸ > Extract
                Selected,
                press Enter, or double-click the row.
  - Several:    Ctrl/Shift-click to mark them, then
                File > Extract ▸ > Extract Selected.
  - Everything: File > Extract ▸ > Extract All.
  - From the right-click menu on a row: Extract.

Each file is written into the output folder under a safe version
of its Commodore name. The status line and a toast report how
many files were written (and how many were skipped, if any could
not be decoded).


----------------------------------------------------------------
 8. EDITING A DISK IMAGE
----------------------------------------------------------------

For the writable sector formats (.d64, .d71, .d81) Sector64 is a
full editor. Changes are made in memory; the title shows
"•modified" until you save.

CREATING A NEW DISK

  File > New ▸. Type a disk name and a two-character ID, then
  click D64, D71, or D81. A freshly formatted, empty image is
  created (unsaved). Use Save As… to write it to a file.

ADDING FILES

  With a disk open, either:
    - Drag host files onto the window (on Linux this needs X11;
      see section 17), or
    - File > Add file ▸, choose the type (prg / seq / usr), then
      Add file… and pick a file.
  The file is given a Commodore name derived from its host name
  (uppercased, 16 characters max). A toast confirms the add.

RENAMING

  Select a file and press F2, or use File > Rename, or the
  right-click Rename. Edit the name in place:
      Enter  commits the new name.
      Esc    (or clicking away) cancels.

DELETING (SCRATCHING)

  Select one or more files and press Delete, or use File >
  Delete, or the right-click Delete. For a disk this scratches
  the file and frees its blocks.

SAVING

  File > Save (Ctrl+S) writes back to the file you opened.
  File > Save As… writes to a new file (and lets you choose the
  format/name). The title drops the "•modified" mark once saved.

UNDO AND REDO

  File > Undo (Ctrl+Z) steps back through your edits to a disk or
  a new archive: adds, renames and deletes, up to the last 32.
  File > Redo (Ctrl+Y, or Ctrl+Shift+Z) puts them back. Hover over
  either menu item to see which edit it would undo or redo; the
  status line says what was undone.

  Making a new edit after undoing clears the redo history.
  Opening another file or starting a new disk or archive clears
  both. Undo works across a save: undoing past it marks the disk
  "•modified" again, since it no longer matches the file, and
  redoing back to the saved state clears the mark.

  While you are typing in a text field (the filter box or a
  rename), Ctrl+Z undoes the typing instead.

THE UNSAVED-CHANGES GUARD

  If a disk has unsaved edits and you try to open another file or
  close the window, Sector64 asks first:
      Save first   — saves, then continues.
      Discard      — throws the edits away and continues.
      Cancel       — stays where you are.
  Closing the window is held until you answer.


----------------------------------------------------------------
 9. BUILDING AN ARCHIVE
----------------------------------------------------------------

  1. File > New ▸ > Empty archive.
  2. Add files by dragging them in (on Linux this needs X11; see
     section 17), or File > Add file ▸.
  3. File > Save archive as… and choose a container format by
     the extension you type:
         .t64   tape image
         .lnx   Lynx
         .lbr   Library
         .p00   PC64 (exactly one file per .p00)

The staging list shows the files you have added and their sizes
before you save.


----------------------------------------------------------------
 10. VALIDATING A DISK
----------------------------------------------------------------

Choose Disk > Validate Disk (or press Ctrl+L) to check an open
disk's integrity — the BAM against the directory chain, block
links, and allocation. The result appears in a centred popover:

    green   — the disk checks out.
    red (⚠) — problems were found, described in the report.

Click anywhere on the popover to dismiss it.


----------------------------------------------------------------
 11. THE DISK MAP
----------------------------------------------------------------

Choose Disk > Disk Map (or press Ctrl+M) to see every
block of the open disk in one picture: a column per track, a row
per sector, each cell coloured by what is stored there. It works
for every disk image, including read-only ones (G64, D2M,
Zip-Code sets).

    green / teal /     a file's blocks, in its type colour
    purple / yellow    (PRG / SEQ / USR / REL), as in the listing
    white              the file selected in the listing
    lavender           the DOS: header, BAM and directory
    dark               free
    amber              allocated in the BAM, but no file uses it
    red                a conflict: a file's block that the BAM
                       marks free (the next file written could
                       overwrite it), or a block two files share

Hover over a block to see its track and sector, and which file
it belongs to ("block 4 of 12"). The rest of that file's blocks
are outlined, so you can see how it is spread across the disk.
Click a block to select its file in the listing.

The line under the title totals each kind. Its free count
includes the directory track, which the header's "blocks free"
figure leaves out, so the two can differ.

If a file's block links are broken (a link off the disk, or a
chain that loops), the map shows as much as it could follow and
lists the problem above the grid. Validate Disk (section 10)
stops at such faults; the map carries on.

Amber blocks aren't always damage: some copy-protected disks and
boot loaders keep data where the directory doesn't point. GEOS
disks are understood: VLIR records and info blocks belong to
their file. The map is rebuilt after every edit, undo and redo.
Esc or Close hides it.


----------------------------------------------------------------
 12. COPYING BETWEEN DISKS
----------------------------------------------------------------

Choose File > Copy... (or press Ctrl+K) to open the Copy
window. It holds a second disk, alongside the one in the main
window, and copies files between the two.

  1. In the Copy window, click Open... to pick the disk to copy
     to (a D64, D71, D81 or CMD-native D2M), or New ▸ to make a
     blank D64, D71 or D81. A new disk takes its name and ID
     from File > New.
  2. In the main list, select the files to copy (Ctrl-click or
     Shift-click for several), then click Copy Here.

The main window can show any disk or archive, read-only ones
included (G64, D2M, Zip-Code, T64, ARC...), so this is also how
you put files from an archive onto a disk. The second disk stays
in the Copy window while you open other files in the main
window, so you can gather files from several sources onto one
disk.

To copy the other way, select files in the Copy window's list
and click Copy to Open Disk. That needs an editable disk (or a
new archive) in the main window, and the main Undo takes it back.

Copies keep each file's name exactly, its type and its lock
flag. Files that can't be copied are skipped, and the status
line says why:
  - a file of the same name is already on the disk
  - the disk is full
  - REL files: their side sectors belong to the disk they're on
  - GEOS files on a GEOS disk: a plain copy would lose the info
    block and VLIR records, which GEOS needs

The Copy window's disk has its own Save, Save As..., Undo (for
the copies onto it) and Close Disk buttons. While it has unsaved
changes, Open... and New ▸ are disabled, and Close Disk becomes
Discard. Quitting with unsaved changes on either disk asks first.
Esc or Close hides the window without closing its disk.


----------------------------------------------------------------
 13. RUNNING IN VICE
----------------------------------------------------------------

Choose File > Run in VICE (or press Ctrl+R) to try a
program in the VICE emulator, which you install separately
(vice-emu.sourceforge.io, or your Linux distribution's "vice"
package).

  - With a disk open, VICE starts with that disk in drive 8 and
    loads the selected program, or the disk's first program if
    the selection isn't one. The program can load the rest of
    its files from the disk, as on a real C64.
  - From an archive or a single file (T64, ARC, P00, PRG...),
    select a program and VICE runs just that program.

VICE gets a copy, written to a temporary folder, so:
  - unsaved edits are included: you can add a file and try it
    before you save, and
  - nothing VICE does, such as a game saving its high scores,
    changes your own file.

A program that loads where the C128 keeps BASIC ($1C01) runs in
VICE's C128 emulator (x128); everything else runs on a C64
(x64sc).

Sector64 looks for VICE on your PATH. If it isn't there (usual on
Windows), it asks for the folder VICE is in, and remembers it.
VICE types the LOAD command for you; characters in a name that
can't be typed are sent as the ? wildcard, so a disk with two
names that differ only there may load the wrong one.


----------------------------------------------------------------
 14. STATUS LINE AND TOAST MESSAGES
----------------------------------------------------------------

The status line at the bottom always shows the result of your
last action. The same outcomes also pop up as toast cards at the
upper-right, tagged by kind:

    OK    (green)  — success: loaded, saved, extracted, renamed.
    ERR   (red)    — something failed; read the message.
    i     (accent) — a hint, e.g. "Select a file first".

Toasts fade out after a few seconds; click one to dismiss it
early. The most recent few are shown at once.


----------------------------------------------------------------
 15. KEYBOARD SHORTCUTS
----------------------------------------------------------------

    Ctrl+O      Open a file
    Ctrl+S      Save the open disk
    Ctrl+Z      Undo the last edit
    Ctrl+Y      Redo (also Ctrl+Shift+Z)
    Ctrl+L      Validate the open disk
    Ctrl+M      Show or hide the disk map
    Ctrl+K      Show or hide the Copy window
    Ctrl+R      Run the disk or selected program in VICE
    Enter       Extract the selected file
    Delete      Delete / scratch the selected file(s)
    F2          Rename the selected file (in place)
    Up / Down   Move the selection
    Esc         Cancel an in-place rename; close the disk map
                or the Copy window

Single-key shortcuts are ignored while you are typing in a text
field (the filter box or a rename).


----------------------------------------------------------------
 16. SETTINGS THAT PERSIST
----------------------------------------------------------------

Between runs Sector64 remembers:
    - the window size (and position, where the window manager
      allows it), and
    - the Output folder you last chose, and
    - your recent files (see 4), and
    - the VICE folder, if you had to choose it (see 13).

Note: under a tiling Wayland compositor the window manager
controls window geometry, so the saved size may be overridden by
your layout. The output folder is always restored.


----------------------------------------------------------------
 17. TIPS AND TROUBLESHOOTING
----------------------------------------------------------------

THE PETSCII FONT

  Sector64 renders in the authentic "C64 Pro Mono" PETSCII font
  if it is installed on your system; otherwise it falls back to
  your default monospace font. The font is not bundled (its
  licence does not allow redistribution). To get the genuine
  look, install C64 Pro Mono from Style's website and restart.

NOTHING HAPPENS WHEN I EXTRACT

  Make sure you have set an Output folder (the Output: button at
  the bottom). The status line / a toast will tell you where the
  files went.

DRAG-AND-DROP ON LINUX (WAYLAND)

  On a Wayland desktop (GNOME, KDE Plasma, Hyprland, Sway…)
  Sector64 runs as a native Wayland window, and files dropped on
  it never arrive: the windowing library it uses does not yet
  support drag-and-drop on Wayland. Use File > Open… and
  File > Add file ▸ instead, or start Sector64 through X11:

      SECTOR64_X11=1 sector64-gui

  On Wayland this runs the window through XWayland, where
  drag-and-drop works. The catch: with display scaling above
  100% an XWayland window may look slightly blurry. Set the
  variable to 0 (or leave it unset) for the native Wayland
  window. Windows and X11 desktops always support drag-and-drop.

A FILE SHOWS "NOT EXTRACTABLE"

  That member uses a compression method Sector64 does not yet
  decode. It is listed for reference, but its bytes cannot be
  written out.

================================================================
 Sector64 — a clean-room, zero-dependency Commodore disk and
 archive toolkit. This manual covers the graphical browser; the
 command-line tools (s64ls, s64extract, s64arc, and friends)
 cover the same formats from a shell.
================================================================
