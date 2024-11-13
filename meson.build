project(
  'stage',
  'rust',
  version: '0.1.4',
  license: 'GPL-3.0-or-later'
)
build_script = find_program(join_paths(meson.source_root(), 'install.sh'))
sources = ['Cargo.toml']
cargo_release = custom_target(
  'cargo-build',
  build_by_default: true,
  input: sources,
  output: meson.project_name(),
  console: true,
  install: false,
  command: [
    build_script,
    meson.source_root(),
  ]
)