# Contracts — shy-notes

All coordinates in the core are **physical pixels** unless noted.

## MouseSample

```text
{ t_ms: u64, x_phys: f64, y_phys: f64 }
```

## MonitorInfo

```text
{ id: String, x: f64, y: f64, w: f64, h: f64, scale: f64 }
```

## DesktopLayout

```text
{ monitors: MonitorInfo[] }
```

`safe_union` is derived as the union of monitor rectangles.

## WidgetPose

```text
{ x: f64, y: f64, w: f64, h: f64, pinned: bool }
```

## InteractionParams

Centralized tunables (defaults; adjust in Phase 7):

| Field | Default |
| ----- | ------- |
| capture_diameter | 100 |
| influence_radius | 900 |
| pre_capture_delay_ms | 400 |
| look_ahead_px | 150 |
| still_speed_threshold | 12 |
| repulsion_strength | 2.6 |
| repulsion_max_step | 320 |
| animation_lerp | 1.0 |

## InteractionCommand

```text
Noop
SetPose { x, y }
SetVisual { pre_capture: bool, glow: f32 }
RequestFocus
SuppressRepulsion
```

## AppStateSnapshot

```text
{ version: 1, text: String, pose: WidgetPose, pinned: bool }
```

## State machine

`Idle | Evaluating | PreCapture | Captured | Repelling | Dragging | Pinned`

Bias: when intent is ambiguous, do not move.
