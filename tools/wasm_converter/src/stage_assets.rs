//! Stage text assets shared by native probes and the WASM converter. OBJ/MTL
//! geometry is reconstructed from ISO archives; verified descriptors and the
//! seven model-rendered sky images are literals in the version-locked recipes.

use std::collections::BTreeMap;

use crate::{
    stage_galaxy::StageGalaxy,
    stage_geometry::{self, GeometryPool, StageGeometryError},
    stage_obj::{self, StageObjError},
    stage_recipe::{RecipeError, StageRecipe},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageAssetsError {
    Geometry(StageGeometryError),
    Object(StageObjError),
    Recipe(RecipeError),
    MissingObject,
}

impl From<StageGeometryError> for StageAssetsError {
    fn from(error: StageGeometryError) -> Self {
        Self::Geometry(error)
    }
}

impl From<StageObjError> for StageAssetsError {
    fn from(error: StageObjError) -> Self {
        Self::Object(error)
    }
}

impl From<RecipeError> for StageAssetsError {
    fn from(error: RecipeError) -> Self {
        Self::Recipe(error)
    }
}

/// Reconstruct the text assets in a stage pack.
///
/// `archive` receives a game-relative path such as `ObjectData/Example.arc`.
/// Source archives are loaded for the geometry pass and then released by the
/// caller's provider. Output paths match the native construction tree.
pub fn geometry_files(
    recipe: &StageRecipe,
    stage: &StageGalaxy,
    mut archive: impl FnMut(&str) -> Option<Vec<u8>>,
) -> Result<Vec<(String, Vec<u8>)>, StageAssetsError> {
    let pool = stage_geometry::build_pool(stage, &recipe.sources, |name| {
        archive(&format!("ObjectData/{name}.arc"))
    })?;
    let mut outputs = decode_geometry_files(recipe, stage, &pool, &mut archive)?;
    for file in &recipe.files {
        if file.name.ends_with(".galaxy") || file.name.ends_with(".stage") {
            let bytes = recipe.literal_file(&file.name)?;
            let path = format!(
                "assets/stages/{}/{}",
                recipe
                    .pack_name()
                    .trim_end_matches(".pak")
                    .trim_start_matches("stages/"),
                file.name
            );
            outputs.push((path, bytes));
        }
    }
    match recipe.literal_file("sky.png") {
        Ok(bytes) => {
            let path = format!(
                "assets/stages/{}/sky.png",
                recipe
                    .pack_name()
                    .trim_end_matches(".pak")
                    .trim_start_matches("stages/")
            );
            outputs.push((path, bytes));
        }
        Err(RecipeError::FileMissing) => {}
        Err(error) => return Err(error.into()),
    }
    Ok(outputs)
}

fn decode_geometry_files(
    recipe: &StageRecipe,
    stage: &StageGalaxy,
    pool: &GeometryPool,
    archive: &mut impl FnMut(&str) -> Option<Vec<u8>>,
) -> Result<Vec<(String, Vec<u8>)>, StageAssetsError> {
    let candidates = stage_obj::name_candidates(pool);
    let mut repeats_cache = BTreeMap::<usize, BTreeMap<String, f64>>::new();
    let mut outputs = Vec::new();
    for file in &recipe.files {
        let decoded = if file.name.ends_with(".obj") {
            stage_obj::decode_file_with_repeats(
                recipe,
                &file.name,
                pool,
                &stage.objects,
                |object_index| {
                    if let Some(repeats) = repeats_cache.get(&object_index) {
                        return Ok(repeats.clone());
                    }
                    let object = stage
                        .objects
                        .get(object_index)
                        .ok_or(StageObjError::InvalidObjectReference)?;
                    let bytes = archive(&format!("ObjectData/{}.arc", object.name));
                    let repeats = match bytes {
                        Some(bytes) => stage_geometry::texture_repeats(&bytes, object)
                            .map_err(|_| StageObjError::InvalidObjectReference)?,
                        None => BTreeMap::new(),
                    };
                    repeats_cache.insert(object_index, repeats.clone());
                    Ok(repeats)
                },
            )?
        } else if file.name.ends_with(".mtl") {
            recipe.decode_file(&file.name, &candidates)?
        } else {
            continue;
        };
        let path = format!(
            "assets/stages/{}/{}",
            recipe
                .pack_name()
                .trim_end_matches(".pak")
                .trim_start_matches("stages/"),
            file.name
        );
        outputs.push((path, decoded.join("\n").into_bytes()));
    }
    Ok(outputs)
}
