# DANGOMUSHI Native UI System

## Purpose

The native UI system keeps DANGOMUSHI visually consistent as new tools are added without turning each screen into a separate styling implementation.

The implementation lives in `src/native_ui_theme.rs`.

## Design rules

- square edges; corner radius is zero,
- one-pixel structural borders,
- dark neutral surfaces,
- semantic color is reserved for state,
- monospace text remains the default visual language,
- spacing comes from a small shared token set,
- icons are monochrome and understandable at very small sizes,
- no gradients, external icon packs, hosted fonts or runtime asset downloads.

## Shared primitives

The first shared primitives are:

- `panel`
- `panel_alt`
- `section_title`
- `metric`
- `pixel_button`
- `status_badge`
- `key_value`
- `small_label`
- `pixel_icon`

Future tools should use these primitives before introducing screen-local styling.

## Original pixel icons

Icons are 5x5 source-code bitmaps drawn as filled rectangles through egui. They are not copied from an external icon library.

Current vocabulary:

~~~text
HOME
TOOLS
FILES
EDITOR
WORKSPACE
MOBILE
SYSTEM
HASH
COMPARE
REPOSITORY
BACKUP
FOLDER
FILE
LINK
OTHER
~~~

The small grid is intentional. It keeps icons legible in the terminal/pixel visual language and avoids shipping raster assets that would need separate scaling and integrity handling.

## State colors

~~~text
TEXT   primary content
MUTED  secondary / disabled
OK     ready / safe / directory
WARN   dirty / recovery / symlink
BAD    errors / failed state
~~~

Color is supplementary. Important states also remain visible as text labels such as READY, PLANNED, MODIFIED and FAILED.
