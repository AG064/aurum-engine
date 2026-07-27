extends Node

# Wrapper around the native `Mavis` class.
#
# In Godot 4.7, GDExtension classes are available as native classes. The
# autoload's `_engine = Mavis.new()` should instantiate the native class
# directly. This wrapper file exists so the rest of the project can
# `preload` a script and don't depend on the extension being loaded at
# the preload step (which would fail in the editor before the add-on
# is enabled).
#
# At runtime, this script's `_init` is replaced by the native class. The
# editor shows the native class instead of this script.

# This file is intentionally minimal. The Mavis class is provided by
# aurum-godot.dll via the GDExtension at addons/aurum/bin/aurum.gdextension.
