================================================================
 SECTOR64 — USER MANUAL
================================================================

Sector64 is a native tool for browsing, extracting from, and
editing Commodore disk images and archives — a DirMaster-style
workbench for the C64 world, built on a dependency-free Rust
core.

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
  6. The preview pane (HEX / TEXT)
  7. Extracting files
  8. Editing a disk image
       - Creating a new disk
       - Adding files
       - Renaming
       - Deleting (scratching)
       - Saving
       - The unsaved-changes guard
  9. Building an archive
 10. Validating a disk
 11. Status line and toast messages
 12. Keyboard shortcuts
 13. Settings that persist
 14. Tips and troubleshooting


----------------------------------------------------------------
 1. GETTING STARTED
----------------------------------------------------------------

Launch the browser from a terminal:

    sector64-gui

You can also open a file straight away by passing its path:

    sector64-gui disk.d64
    sector64-gui "MY ARCHIVE.ARC"

The window remembers its size and your chosen output folder
between runs, so it comes back the way you left it.


----------------------------------------------------------------
 2. SUPPORTED FORMATS
----------------------------------------------------------------

Disk images (read):
    .d64  .d71  .d81  .g64  .d2m

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
fly. A member that uses a decoder Sector64 does not implement is
listed with the reason in place of its bytes, instead of failing
the whole archive.


----------------------------------------------------------------
 3. THE WINDOW AT A GLANCE
----------------------------------------------------------------

Default layout (the preview pane is hidden until you turn it on):

  +----------------------------------------------------------+
  | ◆ SECTOR64  File  Extract | Validate Disk  Preview  Help |  <- menu bar
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
  | ◆ SECTOR64  File  Extract | Validate Disk [Preview] Help |  <- Preview lit
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

- Menu bar (top): File, Extract, a Validate Disk button, a
  Preview toggle, and Help.
- Header row: the disk/archive title and, for disks, blocks
  free. Shown as the first line of the listing.
- Filter box: live name filter.
- Listing: the files, in columns, with colour-coded badges. It
  uses the full width until the preview pane is shown.
- Preview pane (right): hidden by default; turn it on with the
  Preview menu item. Shows the selected file's bytes as hex or
  text. Drag its left edge to resize it.
- Bottom bar: the opened file path, the output folder, and the
  status line.
- Toasts: brief messages pop in at the upper-right.


----------------------------------------------------------------
 4. OPENING A FILE
----------------------------------------------------------------

Three ways:

  - Menu:  File > Open…  (or press Ctrl+O), then pick a file.
  - Launch argument:  sector64-gui <path>
  - Drag a disk image or archive onto the window.

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
 6. THE PREVIEW PANE (HEX / TEXT)
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

Drag the pane's left edge to make it wider or narrower. Files
that cannot be decoded (an unsupported archive member) show the
reason here instead of bytes.


----------------------------------------------------------------
 7. EXTRACTING FILES
----------------------------------------------------------------

First set where extracted files should go: click the Output:
button at the bottom and pick a folder. (It is pre-filled with
the folder your file came from, and it is remembered between
runs.)

Then extract:

  - One file:   select it and choose Extract > Extract Selected,
                press Enter, or double-click the row.
  - Several:    Ctrl/Shift-click to mark them, then
                Extract > Extract Selected.
  - Everything: Extract > Extract All.
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
    - Drag host files onto the window, or
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
  2. Add files by dragging them in, or File > Add file ▸.
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

Click Validate Disk in the menu bar to check an open disk's
integrity — the BAM against the directory chain, block links,
and allocation. The result appears in a centred popover:

    green   — the disk checks out.
    red (⚠) — problems were found, described in the report.

Click anywhere on the popover to dismiss it.


----------------------------------------------------------------
 11. STATUS LINE AND TOAST MESSAGES
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
 12. KEYBOARD SHORTCUTS
----------------------------------------------------------------

    Ctrl+O      Open a file
    Ctrl+S      Save the open disk
    Enter       Extract the selected file
    Delete      Delete / scratch the selected file(s)
    F2          Rename the selected file (in place)
    Up / Down   Move the selection
    Esc         Cancel an in-place rename

Single-key shortcuts are ignored while you are typing in a text
field (the filter box or a rename).


----------------------------------------------------------------
 13. SETTINGS THAT PERSIST
----------------------------------------------------------------

Between runs Sector64 remembers:
    - the window size (and position, where the window manager
      allows it), and
    - the Output folder you last chose.

Note: under a tiling Wayland compositor the window manager
controls window geometry, so the saved size may be overridden by
your layout. The output folder is always restored.


----------------------------------------------------------------
 14. TIPS AND TROUBLESHOOTING
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
