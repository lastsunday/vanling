+++
title = "Vanling"
[extra]
source_file_hash = "2d77c61365fb9981ee0a39128e90c3c415690075"
translated_at = "2026-07-30T00:00:00Z"
+++

# Vanling

> [!WARNING]
> This project is being developed, all the things is not stable.

[![CI](https://github.com/lastsunday/vanling/actions/workflows/ci.yml/badge.svg)](https://github.com/lastsunday/vanling/actions/workflows/ci.yml)
[![app-dev-release](https://github.com/lastsunday/vanling/actions/workflows/app-dev-release.yml/badge.svg)](https://github.com/lastsunday/vanling/actions/workflows/app-dev-release.yml)
[![app-release](https://github.com/lastsunday/vanling/actions/workflows/app-release.yml/badge.svg)](https://github.com/lastsunday/vanling/actions/workflows/app-release.yml)
[![server-dev-release](https://github.com/lastsunday/vanling/actions/workflows/server-dev-release.yml/badge.svg)](https://github.com/lastsunday/vanling/actions/workflows/server-dev-release.yml)
[![server-release](https://github.com/lastsunday/vanling/actions/workflows/server-release.yml/badge.svg)](https://github.com/lastsunday/vanling/actions/workflows/server-release.yml)

[![GitHub Release](https://img.shields.io/github/v/release/lastsunday/vanling)](https://github.com/lastsunday/vanling/releases)
[![Docker](https://img.shields.io/badge/Docker-2496ED?logo=docker&logoColor=fff)](https://hub.docker.com/r/lastsunday/vanling/tags)

## Purpose

- Learn the Rust programming language, voice interaction, and large language model technologies.
- Build a fully self-hosted chatbot (including all components such as LLM, TTS, etc.), similar to a self-hosted server solution for [xiaozhi-esp32](https://github.com/78/xiaozhi-esp32).

## More Info

<details>
<summary>Want an interface overview? Click to expand</summary>

### Login / Registration Page

**_TODO_**

### User Dashboard

6 StatCards, TrendsChart, LatencyChart, LatencyTable, RecentSessionsTable

</details>

## Features

- [x] Connection: WebSocket
- [x] Voice Interaction: VAD, ASR, TTS
- [x] Conversation: LLM
- [x] MCP: Self-hosted/Remote Server MCP, Device MCP
- [ ] Admin
  1. Dashboard (implemented: StatCards/TrendsChart/Latency Analysis/Recent Sessions)
  1. Management console (in development)
  1. Web-based device simulator (in development)
- [ ] Deployment: Binary (in development), Docker (in development)
- [ ] Compatible Devices
  1. [xiaozhi-esp32](https://github.com/78/xiaozhi-esp32) (in development)
  1. vanling (Flutter cross-platform App, in development)

## System Requirements

_TODO_
