# ADR 0001: Build from scratch; own the core, rent platform layers

- Status: accepted, 2026-11-24
- Source: direction doc, "Why build from scratch" and guardrail 1

## Context
Bevy's fast API churn, general-purpose scope and human-first design work against coding agents: APIs an
agent learned last month are gone, and the engine's surface is far larger than learning games need.
Rewriting everything, though, would sink a small team: the platform layers (GPU, windowing and app
lifecycles on iOS/Android, web audio, complex-script text shaping) hold most of the platform pain.

## Decision
Own the core runtime, the data model and the agent interface. Rent wgpu, winit, audio and text shaping
when a reference game needs them. In v0.1 the only rented crates are `toml_edit` (parsing) and `png`.

## Consequences
- The agent-facing surface (file formats, CLI, diagnostics, determinism) is fully under our control and
  can be designed for agents from the first line.
- We carry the cost of writing a simulation, a renderer front end and tooling ourselves; scope is kept
  small by guardrail 2 (reference games drive features).

## Revisit when
A rented layer we need does not exist or is unmaintained, or owning a layer stops paying for itself.
