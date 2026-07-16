{
  description = "release train language-family-owner-fix (42968e1ab044c97cacd3cb641d79825f5a48085b6fbc8ebbba1458fb4fbf266d)";
  inputs = {
    nota.url = "github:LiGoldragon/nota/ad5c3a707cbd012708a979febb15822c922a060b";
    schema_language.url = "github:LiGoldragon/schema-language/32e710032b2b78ce26cebf898efba815f82a3cc0";
    schema_rust.url = "github:LiGoldragon/schema-rust/33065a00f40dcdc36398bb7eaa370e9f7c91401f";
  };
  outputs = inputs:
    let
      systems = [ "aarch64-darwin" "aarch64-linux" "x86_64-darwin" "x86_64-linux" ];
      releaseTrain = builtins.fromJSON (builtins.readFile ./release-train.lock.json);
      components = builtins.removeAttrs inputs [ "self" ];
      componentPackages = system: builtins.mapAttrs (_: component: component.packages.${system}.default) components;
      perSystem = builtins.listToAttrs (map (system: { name = system; value = componentPackages system; }) systems);
    in {
      inherit releaseTrain;
      packages = perSystem;
      checks = perSystem;
    };
}
