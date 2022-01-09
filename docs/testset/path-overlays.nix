let
  try = x: def: let res = builtins.tryEval x; in if res.success then res.value else def;
in
  try (toString <nixpkgs-overlays>) ""
