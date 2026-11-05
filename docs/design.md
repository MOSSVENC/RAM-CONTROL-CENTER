# Neo Control visual direction

## Concept

Neo Control is a hardware instrument, not a generic settings dashboard. The UI should feel like a precise piece of laptop equipment: cool graphite, restrained signal colour, stable typography and dense but legible telemetry.

## Tokens

- Base: `#06090D`
- Rail: `#0A0F15`
- Instrument surface: `#0D131A`
- Raised surface: `#121A23`
- Divider: `#1D2834`
- Primary text: `#EDF3F8`
- Secondary text: `#8B9AAB`
- Telemetry cyan: `#78C9D4`
- Signal amber: `#E6A85C`
- Confirmed green: `#7FC9A3`
- Fault red: `#DF7F78`

## Type

- Bahnschrift carries headings, labels and descriptive copy.
- Cascadia Mono carries temperatures, clocks, fan speeds, percentages and command identifiers.
- The title is centred, uppercase and spaced with ordinary spaces: `NEO CONTROL`.

## Layout

```text
┌────────────┬──────────────────────────────────────────────┐
│ pixel N    │                 NEO CONTROL                  │
│            ├──────────────────────────────────────────────┤
│ Overview   │  page heading                                │
│ Performance│  ┌──────────────────────────────┬───────────┐ │
│ Cooling    │  │ primary instrument           │ live side │ │
│            │  │ large thermal/fan figures    │ readings  │ │
│            │  └──────────────────────────────┴───────────┘ │
│Bridge state│  compute / resources / profile strip         │
└────────────┴──────────────────────────────────────────────┘
```

Overview is read-only. Performance and Cooling own their controls. No identical card grid is used as the page structure; surfaces differ by hierarchy and purpose.

## Motion

- 120–140 ms colour transitions for navigation and controls.
- 260 ms width animation for telemetry bars.
- No infinite glow, rotating fan, parallax or decorative loop.
- Thermal sampling runs every 2 seconds and extended reads every other cycle, so motion never forces extra AcpiBridge traffic.

## Renderer

Slint uses its winit + FemtoVG OpenGL renderer. This is a GPU path; the executable can be assigned a high-performance adapter in Windows graphics settings if the laptop exposes multiple GPUs.

## Temperature units

All temperatures are displayed explicitly in degrees Celsius (`°C`). A bare degree glyph is not used as a unit.
