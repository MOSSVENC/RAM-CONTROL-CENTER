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

- Navigation uses a two-stage fade-through: 72 ms exit, an instant swap at zero
  opacity, then 190 ms fade and 240 ms rise/scale on the Material emphasized
  curve `cubic-bezier(0.2, 0, 0, 1)`.
- Option surfaces morph over 200–220 ms. Selection bars and radio dots use a
  240 ms `ease-out-back` settle. Press feedback scales controls to 0.97 and
  releases over 260 ms.
- Toggle switches slide a 20 px thumb over 240 ms while the track cross-fades
  colour and border state. Telemetry bars retain their 260 ms value animation.
- No infinite glow, rotating fan, parallax, snapshot texture or decorative loop.
- Light system sampling runs every second; thermal and extended vendor reads run
  every five seconds so live motion never forces extra AcpiBridge traffic.

The motion language is cross-checked against two reference implementations:

- Kazumi: 70 ms tab fade, 120 ms side-panel `easeOut` slide, 150/300 ms pressed
  scale (`easeOutCubic`/`easeOutBack`), 220 ms selected surfaces with
  `easeInOutCubicEmphasized`, and scale-plus-fade switchers.
- KernelSU: Compose Material navigation motion, spring-driven segmented rows and
  interactive highlights, animated expand/fade visibility, and expressive
  switch state transitions.

Reference files:

- [Kazumi index_module.dart](https://github.com/Predidit/Kazumi/blob/main/lib/pages/index_module.dart)
- [Kazumi side_panel_transition.dart](https://github.com/Predidit/Kazumi/blob/main/lib/bean/widget/side_panel_transition.dart)
- [Kazumi my_space_view.dart](https://github.com/Predidit/Kazumi/blob/main/lib/pages/my/my_space_view.dart)
- [Kazumi rule_card.dart](https://github.com/Predidit/Kazumi/blob/main/lib/bean/card/rule_card.dart)
- [Kazumi player_adjustment_hud.dart](https://github.com/Predidit/Kazumi/blob/main/lib/pages/player/player_adjustment_hud.dart)
- [KernelSU SegmentedList.kt](https://github.com/tiann/KernelSU/blob/main/manager/app/src/main/java/me/weishu/kernelsu/ui/component/material/SegmentedList.kt)
- [KernelSU ExpressiveSwitch.kt](https://github.com/tiann/KernelSU/blob/main/manager/app/src/main/java/me/weishu/kernelsu/ui/component/material/ExpressiveSwitch.kt)
- [KernelSU InteractiveHighlight.kt](https://github.com/tiann/KernelSU/blob/main/manager/app/src/main/java/me/weishu/kernelsu/ui/component/miuix/animation/InteractiveHighlight.kt)
- [KernelSU MainActivity.kt](https://github.com/tiann/KernelSU/blob/main/manager/app/src/main/java/me/weishu/kernelsu/ui/MainActivity.kt)

## Renderer

Slint uses its winit + FemtoVG OpenGL renderer. This is a GPU path; the executable can be assigned a high-performance adapter in Windows graphics settings if the laptop exposes multiple GPUs.

## Temperature units

All temperatures are displayed explicitly in degrees Celsius (`°C`). A bare degree glyph is not used as a unit.
