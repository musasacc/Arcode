# Development shell for building and running Arcode with Nix / on NixOS.
#
#   nix-shell                              # Arcode itself
#   nix-shell --arg withPlugins true       # also clang + wasi-libc for
#                                          # scripts/build-plugins.sh
{ pkgs ? import <nixpkgs> { }, withPlugins ? false }:

let
  # Arcode (GPUI) opens these with dlopen at run time, so they must be on
  # LD_LIBRARY_PATH: Vulkan, Wayland, and fontconfig for system fonts.
  runtimeLibs = with pkgs; [ vulkan-loader wayland fontconfig ];
  wasilibc = pkgs.pkgsCross.wasi32.wasilibc.dev;
  clang = pkgs.llvmPackages.clang-unwrapped;
in
pkgs.mkShell ({
  nativeBuildInputs = with pkgs; [ rustup pkg-config git ];
  buildInputs = with pkgs; [ libxkbcommon libxcb freetype ];
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
} // pkgs.lib.optionalAttrs withPlugins {
  # The unwrapped clang: Nix's cc wrapper adds host-only flags.
  CC_wasm32_wasip2 = "${clang}/bin/clang";
  AR_wasm32_wasip2 = "${pkgs.llvm}/bin/llvm-ar";
  # nixpkgs 25.11 puts the headers in include/, unstable in
  # include/wasm32-wasip1.
  shellHook = ''
    for d in ${wasilibc}/include/wasm32-wasip1 ${wasilibc}/include; do
      if [ -f "$d/stdio.h" ]; then export WASI_INCLUDE="$d"; break; fi
    done
  '';
})
