use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use arche_package::{PortablePath, SourceTreeEntry};
use same_file::Handle;

use crate::hir::{
    CheckTargetRequest, DependencyExport, HirDefinition, HirDefinitionId, HirDefinitionKind,
    HirModule, HirNamespace, HirScopeEntry, HirVisibility, LinkedTarget, ModuleId,
    PackageExportEntry, PackageExportSurface, ResolvedTargetHir, TargetKind,
};
use crate::source::{Diagnostic, FileId, OpenedSource, SourceSnapshot, Span};
use crate::symbol::{case_fold_nfc, normalize_identifier, Symbol};
use crate::syntax::{
    parse_reader, AstDefinitionKind, AstFile, AstItem, AstMod, AstPath, AstPathRoot, AstUse,
    AstVisibility,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendErrorCode {
    Source,
    Syntax,
    Module,
    Name,
    Visibility,
    Target,
    Migration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendError {
    pub kind: FrontendErrorCode,
    pub diagnostic: Box<Diagnostic>,
    pub files: Vec<PathBuf>,
}

impl fmt::Display for FrontendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.diagnostic.fmt(formatter)
    }
}

impl std::error::Error for FrontendError {}

pub fn check_target(request: CheckTargetRequest) -> Result<ResolvedTargetHir, FrontendError> {
    Frontend::new(request).and_then(Frontend::check)
}

pub(crate) fn check_target_with_dependencies(
    request: CheckTargetRequest,
    dependencies: Vec<DependencyExport>,
) -> Result<ResolvedTargetHir, FrontendError> {
    Frontend::new_with_dependencies(request, dependencies).and_then(Frontend::check)
}

pub(crate) fn package_export_surface(
    hir: &ResolvedTargetHir,
    dependencies: &[DependencyExport],
) -> PackageExportSurface {
    debug_assert_eq!(hir.target().kind, TargetKind::Library);
    let mut public_modules = BTreeMap::from([(hir.target().root_module, true)]);
    for module in hir.modules().iter().skip(1) {
        let parent = module
            .parent
            .expect("every non-root HIR module has a parent");
        let binding = hir.modules()[index(parent)]
            .scopes
            .get(&HirNamespace::Module)
            .and_then(|entries| {
                entries.iter().find(
                    |entry| matches!(entry.kind, HirDefinitionKind::Module(id) if id == module.id),
                )
            })
            .expect("every loaded child module has a parent binding");
        public_modules.insert(
            module.id,
            public_modules.get(&parent).copied().unwrap_or(false)
                && binding.visibility == HirVisibility::Public,
        );
    }

    let mut entries = BTreeMap::new();
    for module in hir.modules() {
        let module_public = public_modules.get(&module.id).copied().unwrap_or(false);
        for bindings in module.scopes.values() {
            for binding in bindings {
                let mut path = module.path.clone();
                path.push(binding.name.clone());
                insert_package_export(
                    &mut entries,
                    PackageExportEntry {
                        path,
                        definition: binding.definition,
                        kind: binding.kind,
                        namespace: binding.namespace,
                        externally_visible: module_public
                            && binding.visibility == HirVisibility::Public
                            && binding.exportable,
                    },
                );
            }
        }
    }

    let mut ancestry = BTreeSet::from([hir.target().root_module]);
    project_public_module(
        hir,
        dependencies,
        hir.target().root_module,
        &[],
        &mut ancestry,
        &mut entries,
    );
    PackageExportSurface {
        package: hir.target().package,
        entries: entries.into_values().collect(),
    }
}

type PackageExportKey = (Vec<Symbol>, HirNamespace, HirDefinitionId);

fn insert_package_export(
    entries: &mut BTreeMap<PackageExportKey, PackageExportEntry>,
    entry: PackageExportEntry,
) {
    let key = (entry.path.clone(), entry.namespace, entry.definition);
    entries
        .entry(key)
        .and_modify(|current| current.externally_visible |= entry.externally_visible)
        .or_insert(entry);
}

fn project_public_module(
    hir: &ResolvedTargetHir,
    dependencies: &[DependencyExport],
    module: ModuleId,
    output_prefix: &[Symbol],
    ancestry: &mut BTreeSet<ModuleId>,
    entries: &mut BTreeMap<PackageExportKey, PackageExportEntry>,
) {
    for bindings in hir.modules()[index(module)].scopes.values() {
        for binding in bindings {
            if binding.visibility != HirVisibility::Public || !binding.exportable {
                continue;
            }
            let mut output_path = output_prefix.to_vec();
            output_path.push(binding.name.clone());
            insert_package_export(
                entries,
                PackageExportEntry {
                    path: output_path.clone(),
                    definition: binding.definition,
                    kind: binding.kind,
                    namespace: binding.namespace,
                    externally_visible: true,
                },
            );

            let HirDefinitionKind::Module(child) = binding.kind else {
                continue;
            };
            if binding.definition.package() == hir.target().package
                && binding.definition.target() == hir.target().id
            {
                if ancestry.insert(child) {
                    project_public_module(
                        hir,
                        dependencies,
                        child,
                        &output_path,
                        ancestry,
                        entries,
                    );
                    ancestry.remove(&child);
                }
            } else {
                project_external_module(dependencies, binding.definition, &output_path, entries);
            }
        }
    }
}

fn project_external_module(
    dependencies: &[DependencyExport],
    definition: HirDefinitionId,
    output_prefix: &[Symbol],
    entries: &mut BTreeMap<PackageExportKey, PackageExportEntry>,
) {
    for surface in dependencies
        .iter()
        .filter_map(|dependency| dependency.surface.as_ref())
    {
        let origins = surface
            .entries
            .iter()
            .filter(|entry| {
                entry.definition == definition
                    && entry.externally_visible
                    && matches!(entry.kind, HirDefinitionKind::Module(_))
            })
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        for origin in origins {
            for descendant in surface.entries.iter().filter(|entry| {
                entry.externally_visible
                    && entry.path.len() > origin.len()
                    && entry.path.starts_with(&origin)
            }) {
                let mut path = output_prefix.to_vec();
                path.extend_from_slice(&descendant.path[origin.len()..]);
                insert_package_export(
                    entries,
                    PackageExportEntry {
                        path,
                        definition: descendant.definition,
                        kind: descendant.kind,
                        namespace: descendant.namespace,
                        externally_visible: true,
                    },
                );
            }
        }
    }
}

struct LoadedModule {
    id: ModuleId,
    parent: Option<ModuleId>,
    name: Option<Symbol>,
    logical_path: Vec<Symbol>,
    file: FileId,
    ast: AstFile,
}

struct SeenPhysicalFile {
    handle: Handle,
    path: PathBuf,
    declaration: Option<Span>,
}

struct DefinitionInput<'a> {
    module: ModuleId,
    name: Symbol,
    kind: HirDefinitionKind,
    visibility: &'a AstVisibility,
    span: Span,
}

struct HirCollection {
    modules: Vec<HirModule>,
    definitions: Vec<HirDefinition>,
    uses: Vec<PendingUse>,
}

struct Frontend {
    request: CheckTargetRequest,
    package_root: PathBuf,
    source_base: PathBuf,
    modules: Vec<LoadedModule>,
    physical_files: Vec<SeenPhysicalFile>,
    files: Vec<PathBuf>,
    source_entries: Vec<SourceTreeEntry>,
    dependency_exports: BTreeMap<String, Option<PackageExportSurface>>,
}

impl Frontend {
    fn new(request: CheckTargetRequest) -> Result<Self, FrontendError> {
        Self::new_with_dependencies(request, Vec::new())
    }

    fn new_with_dependencies(
        request: CheckTargetRequest,
        dependencies: Vec<DependencyExport>,
    ) -> Result<Self, FrontendError> {
        let package_root = fs::canonicalize(&request.package_root).map_err(|error| {
            standalone_error(
                "SOURCE001",
                format!(
                    "could not resolve package root {}: {error}",
                    request.package_root.display()
                ),
            )
        })?;
        let source_path =
            resolve_target_source(&package_root, &request.package_root, &request.source_root)?;
        let source_base = source_path
            .parent()
            .ok_or_else(|| standalone_error("SOURCE001", "target source has no parent directory"))?
            .to_path_buf();

        let mut dependency_exports = BTreeMap::new();
        let mut folded_aliases = BTreeMap::new();
        for raw in &request.dependency_aliases {
            let normalized = normalize_identifier(raw).map_err(|error| {
                standalone_error(
                    "NAME007",
                    format!("invalid dependency alias `{raw}`: {error}"),
                )
            })?;
            if dependency_exports
                .insert(normalized.clone(), None)
                .is_some()
            {
                return Err(standalone_error(
                    "NAME007",
                    format!("duplicate dependency alias `{normalized}`"),
                ));
            }
            let folded = case_fold_nfc(&normalized);
            if let Some(previous) = folded_aliases.insert(folded, normalized.clone()) {
                return Err(standalone_error(
                    "NAME007",
                    format!(
                        "dependency aliases `{previous}` and `{normalized}` collide under full Unicode case folding"
                    ),
                ));
            }
        }
        for dependency in dependencies {
            let alias = normalize_identifier(&dependency.alias).map_err(|error| {
                standalone_error(
                    "NAME007",
                    format!(
                        "invalid dependency export alias `{}`: {error}",
                        dependency.alias
                    ),
                )
            })?;
            let slot = dependency_exports.get_mut(&alias).ok_or_else(|| {
                standalone_error(
                    "NAME007",
                    format!("dependency export `{alias}` was not declared by this package"),
                )
            })?;
            if slot.is_some() {
                return Err(standalone_error(
                    "NAME007",
                    format!("duplicate dependency export `{alias}`"),
                ));
            }
            *slot = dependency.surface;
        }

        let mut frontend = Self {
            request,
            package_root,
            source_base,
            modules: Vec::new(),
            physical_files: Vec::new(),
            files: Vec::new(),
            source_entries: Vec::new(),
            dependency_exports,
        };
        frontend.load_module(source_path, None, None, Vec::new(), None)?;
        Ok(frontend)
    }

    fn check(self) -> Result<ResolvedTargetHir, FrontendError> {
        let mut collection = self.collect_hir()?;
        self.resolve_imports(&mut collection.modules, collection.uses)?;
        let target = self.link_target(&collection.modules, &collection.definitions)?;
        let mut source_entries = self.source_entries;
        source_entries.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(ResolvedTargetHir::new(
            target,
            collection.modules,
            collection.definitions,
            source_entries,
        ))
    }

    fn load_module(
        &mut self,
        path: PathBuf,
        parent: Option<ModuleId>,
        name: Option<Symbol>,
        logical_path: Vec<Symbol>,
        declaration: Option<Span>,
    ) -> Result<ModuleId, FrontendError> {
        let canonical = fs::canonicalize(&path).map_err(|error| {
            self.error(Diagnostic::path(
                "MODULE001",
                format!("could not open module {}: {error}", path.display()),
            ))
        })?;
        if !canonical.starts_with(&self.package_root) {
            return Err(self.error(declaration.map_or_else(
                || {
                    Diagnostic::path(
                        "MODULE005",
                        format!(
                            "module {} resolves outside the package root",
                            path.display()
                        ),
                    )
                },
                |span| {
                    Diagnostic::at(
                        "MODULE005",
                        span,
                        format!(
                            "module {} resolves outside the package root",
                            path.display()
                        ),
                    )
                },
            )));
        }

        let opened =
            OpenedSource::open(&path, &canonical, &self.package_root).map_err(|error| {
                self.error(Diagnostic::path(
                    "SOURCE001",
                    format!("could not securely open source {}: {error}", path.display()),
                ))
            })?;
        if let Some(previous) = self
            .physical_files
            .iter()
            .find(|previous| &previous.handle == opened.identity())
        {
            let diagnostic = declaration.map_or_else(
                || {
                    Diagnostic::path(
                        "MODULE007",
                        format!(
                            "module source {} aliases already loaded {}",
                            canonical.display(),
                            previous.path.display()
                        ),
                    )
                },
                |span| {
                    let diagnostic = Diagnostic::at(
                        "MODULE007",
                        span,
                        format!(
                            "module source {} aliases already loaded {}",
                            canonical.display(),
                            previous.path.display()
                        ),
                    );
                    if let Some(previous_span) = previous.declaration {
                        diagnostic.with_secondary(previous_span, "first loaded here")
                    } else {
                        diagnostic
                    }
                },
            );
            return Err(self.error(diagnostic));
        }

        let file =
            FileId(to_u64(self.files.len(), "source file count").map_err(|d| self.error(d))?);
        self.files.push(canonical.clone());
        let (snapshot, handle) = opened.into_snapshot().map_err(|error| {
            self.error(Diagnostic::path(
                "SOURCE001",
                format!("could not snapshot source {}: {error}", canonical.display()),
            ))
        })?;
        let source_entry = self.source_tree_entry(&canonical, &snapshot)?;
        let ast = parse_reader(
            file,
            snapshot.reader().map_err(|error| {
                self.error(Diagnostic::path(
                    "SOURCE002",
                    format!(
                        "could not read source {}: {error}",
                        snapshot.path().display()
                    ),
                ))
            })?,
        )
        .map_err(|diagnostic| self.error(diagnostic))?;

        let id = ModuleId(to_u64(self.modules.len(), "module count").map_err(|d| self.error(d))?);
        self.source_entries.push(source_entry);
        self.physical_files.push(SeenPhysicalFile {
            handle,
            path: canonical.clone(),
            declaration,
        });
        self.modules.push(LoadedModule {
            id,
            parent,
            name,
            logical_path: logical_path.clone(),
            file,
            ast,
        });

        let declarations = self.modules[index(id)]
            .ast
            .items
            .iter()
            .filter_map(|item| match item {
                AstItem::Mod(module) => Some(module.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut names = BTreeMap::<String, Span>::new();
        for module in declarations {
            if let Some(previous) = names.insert(module.name.as_str().to_owned(), module.name_span)
            {
                return Err(self.error(
                    Diagnostic::at(
                        "MODULE008",
                        module.name_span,
                        format!("duplicate module declaration `{}`", module.name),
                    )
                    .with_secondary(previous, "first declared here"),
                ));
            }
            let child_path = self.child_path(&logical_path, &module)?;
            let mut child_logical = logical_path.clone();
            child_logical.push(module.name.clone());
            self.load_module(
                child_path,
                Some(id),
                Some(module.name),
                child_logical,
                Some(module.name_span),
            )?;
        }
        Ok(id)
    }

    fn source_tree_entry(
        &self,
        canonical: &std::path::Path,
        snapshot: &SourceSnapshot,
    ) -> Result<SourceTreeEntry, FrontendError> {
        let relative = canonical.strip_prefix(&self.package_root).map_err(|_| {
            self.error(Diagnostic::path(
                "SOURCE004",
                format!(
                    "source {} is not beneath package root {}",
                    canonical.display(),
                    self.package_root.display()
                ),
            ))
        })?;
        let mut segments = Vec::new();
        for component in relative.components() {
            let std::path::Component::Normal(segment) = component else {
                return Err(self.error(Diagnostic::path(
                    "SOURCE004",
                    format!("source path {} is not portable", relative.display()),
                )));
            };
            let segment = segment.to_str().ok_or_else(|| {
                self.error(Diagnostic::path(
                    "SOURCE004",
                    format!("source path {} is not valid UTF-8", relative.display()),
                ))
            })?;
            segments.push(segment);
        }
        let portable = PortablePath::new(&segments.join("/")).map_err(|diagnostics| {
            self.error(Diagnostic::path(
                "SOURCE004",
                format!(
                    "source path {} is not a canonical portable path: {diagnostics}",
                    relative.display()
                ),
            ))
        })?;
        Ok(SourceTreeEntry {
            path: portable,
            byte_length: snapshot.byte_length(),
            content_digest: snapshot.content_digest(),
        })
    }

    fn child_path(
        &self,
        parent: &[Symbol],
        declaration: &AstMod,
    ) -> Result<PathBuf, FrontendError> {
        let mut directory = self.source_base.clone();
        for segment in parent {
            directory =
                self.exact_module_entry(&directory, segment.as_str(), declaration.name_span)?;
        }
        let expected = format!("{}.arc", declaration.name);
        self.exact_module_entry(&directory, &expected, declaration.name_span)
    }

    fn exact_module_entry(
        &self,
        directory: &Path,
        expected: &str,
        declaration: Span,
    ) -> Result<PathBuf, FrontendError> {
        let expected_nfc =
            unicode_normalization::UnicodeNormalization::nfc(expected).collect::<String>();
        let expected_fold = case_fold_nfc(expected);
        let mut aliases = Vec::new();
        let entries = fs::read_dir(directory).map_err(|error| {
            self.error(Diagnostic::at(
                "MODULE001",
                declaration,
                format!(
                    "could not inspect module directory {}: {error}",
                    directory.display()
                ),
            ))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                self.error(Diagnostic::at(
                    "MODULE001",
                    declaration,
                    format!(
                        "could not inspect module directory {}: {error}",
                        directory.display()
                    ),
                ))
            })?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let nfc =
                unicode_normalization::UnicodeNormalization::nfc(name.as_str()).collect::<String>();
            if nfc == expected_nfc || case_fold_nfc(&name) == expected_fold {
                aliases.push(name);
            }
        }
        aliases.sort();
        if aliases.len() != 1 || aliases[0] != expected {
            let detail = if aliases.is_empty() {
                format!("expected exactly `{}`", directory.join(expected).display())
            } else {
                format!(
                    "expected exact NFC/case spelling `{expected}`; colliding entries: {}",
                    aliases.join(", ")
                )
            };
            return Err(self.error(
                Diagnostic::at("MODULE002", declaration, detail).with_note(
                    "`mod name;` resolves only `name.arc`; `mod.arc`, path attributes, and wildcard discovery are not supported",
                ),
            ));
        }
        Ok(directory.join(expected))
    }

    fn collect_hir(&self) -> Result<HirCollection, FrontendError> {
        let mut hir_modules = self
            .modules
            .iter()
            .map(|module| HirModule {
                id: module.id,
                parent: module.parent,
                name: module.name.clone(),
                path: module.logical_path.clone(),
                file: module.file,
                scopes: BTreeMap::new(),
            })
            .collect::<Vec<_>>();
        let mut definitions = Vec::new();
        let mut uses = Vec::new();

        for loaded in &self.modules {
            for item in &loaded.ast.items {
                match item {
                    AstItem::Use(import) => uses.push(PendingUse {
                        module: loaded.id,
                        import: import.clone(),
                    }),
                    AstItem::Mod(module) => {
                        let child = self
                            .modules
                            .iter()
                            .find(|candidate| {
                                candidate.parent == Some(loaded.id)
                                    && candidate.name.as_ref() == Some(&module.name)
                            })
                            .expect("module loader created every declared child");
                        self.push_definition(
                            &mut hir_modules,
                            &mut definitions,
                            DefinitionInput {
                                module: loaded.id,
                                name: module.name.clone(),
                                kind: HirDefinitionKind::Module(child.id),
                                visibility: &module.visibility,
                                span: module.name_span,
                            },
                        )?;
                    }
                    AstItem::Definition(definition) => {
                        self.push_definition(
                            &mut hir_modules,
                            &mut definitions,
                            DefinitionInput {
                                module: loaded.id,
                                name: definition.name.clone(),
                                kind: hir_kind(definition.kind),
                                visibility: &definition.visibility,
                                span: definition.span,
                            },
                        )?;
                    }
                }
            }
        }
        Ok(HirCollection {
            modules: hir_modules,
            definitions,
            uses,
        })
    }

    fn push_definition(
        &self,
        modules: &mut [HirModule],
        definitions: &mut Vec<HirDefinition>,
        input: DefinitionInput<'_>,
    ) -> Result<(), FrontendError> {
        let id = HirDefinitionId::new(
            self.request.package,
            self.request.target_id,
            to_u64(definitions.len(), "HIR definition count").map_err(|d| self.error(d))?,
        );
        let namespace = input.kind.namespace();
        let visibility = self.resolve_visibility(input.module, input.visibility)?;
        let entries = modules[index(input.module)]
            .scopes
            .entry(namespace)
            .or_default();
        if let Some(previous) = entries.iter().find(|entry| entry.name == input.name) {
            return Err(self.error(
                Diagnostic::at(
                    "NAME001",
                    input.span,
                    format!("duplicate {:?} binding `{}`", namespace, input.name),
                )
                .with_secondary(previous.span, "first bound here"),
            ));
        }
        if input.module == ModuleId(0)
            && namespace == HirNamespace::Module
            && self
                .dependency_exports
                .keys()
                .any(|alias| case_fold_nfc(alias) == case_fold_nfc(input.name.as_str()))
        {
            return Err(self.error(Diagnostic::at(
                "NAME007",
                input.span,
                format!(
                    "module `{}` conflicts with dependency alias `{}`",
                    input.name, input.name
                ),
            )));
        }
        entries.push(HirScopeEntry {
            name: input.name.clone(),
            definition: id,
            kind: input.kind,
            namespace,
            visibility,
            exportable: visibility == HirVisibility::Public,
            span: input.span,
            imported: false,
        });
        definitions.push(HirDefinition {
            id,
            module: input.module,
            name: input.name,
            kind: input.kind,
            visibility,
            span: input.span,
        });
        Ok(())
    }

    fn resolve_visibility(
        &self,
        module: ModuleId,
        visibility: &AstVisibility,
    ) -> Result<HirVisibility, FrontendError> {
        match visibility {
            AstVisibility::Private => Ok(HirVisibility::Module(module)),
            AstVisibility::Public => Ok(HirVisibility::Public),
            AstVisibility::Package => Ok(HirVisibility::Package),
            AstVisibility::Super => self.modules[index(module)]
                .parent
                .map(HirVisibility::Module)
                .ok_or_else(|| {
                    self.error(Diagnostic::at(
                        "VISIBILITY001",
                        self.modules[index(module)].ast.eof_span,
                        "`pub(super)` is invalid in the target root module",
                    ))
                }),
            AstVisibility::In(path) => {
                let boundary = self.resolve_module_path(module, path)?;
                if !self.is_descendant(module, boundary) {
                    return Err(self.error(Diagnostic::at(
                        "VISIBILITY002",
                        path.span,
                        "`pub(in path)` must name the declaring module or one of its ancestors",
                    )));
                }
                Ok(HirVisibility::Module(boundary))
            }
        }
    }

    fn resolve_module_path(
        &self,
        from: ModuleId,
        path: &AstPath,
    ) -> Result<ModuleId, FrontendError> {
        let mut current = match path.root {
            AstPathRoot::Package => ModuleId(0),
            AstPathRoot::SelfValue => from,
            AstPathRoot::Super(count) => self.ascend(from, count).ok_or_else(|| {
                self.error(Diagnostic::at(
                    "NAME002",
                    path.span,
                    "module path uses `super` above the target root",
                ))
            })?,
            AstPathRoot::Bare => {
                return Err(self.error(Diagnostic::at(
                    "NAME002",
                    path.span,
                    "`pub(in path)` must begin with `package::`, `self::`, or `super::`",
                )));
            }
        };
        for segment in &path.segments {
            current = self
                .modules
                .iter()
                .find(|module| {
                    module.parent == Some(current) && module.name.as_ref() == Some(segment)
                })
                .map(|module| module.id)
                .ok_or_else(|| {
                    self.error(Diagnostic::at(
                        "NAME002",
                        path.span,
                        format!("unknown module segment `{segment}`"),
                    ))
                })?;
        }
        Ok(current)
    }

    fn resolve_imports(
        &self,
        modules: &mut [HirModule],
        mut pending: Vec<PendingUse>,
    ) -> Result<(), FrontendError> {
        while !pending.is_empty() {
            let mut next = Vec::new();
            let mut progressed = false;
            for use_item in pending {
                let Some(bindings) =
                    self.resolve_hir_path(modules, use_item.module, &use_item.import.path)?
                else {
                    next.push(use_item);
                    continue;
                };
                let name = use_item
                    .import
                    .path
                    .segments
                    .last()
                    .expect("parser requires path segment")
                    .clone();
                let visibility =
                    self.resolve_visibility(use_item.module, &use_item.import.visibility)?;
                if bindings.iter().any(|binding| {
                    !self.visibility_is_subset(visibility, binding.visibility)
                        || (visibility == HirVisibility::Public && !binding.exportable)
                }) {
                    return Err(self.error(Diagnostic::at(
                        "VISIBILITY004",
                        use_item.import.span,
                        "an import visibility cannot widen the declaration it exposes",
                    )));
                }
                for binding in &bindings {
                    if let Some(previous) = modules[index(use_item.module)]
                        .scopes
                        .get(&binding.namespace)
                        .and_then(|entries| entries.iter().find(|entry| entry.name == name))
                    {
                        return Err(self.error(
                            Diagnostic::at(
                                "NAME001",
                                use_item.import.span,
                                format!(
                                    "import duplicates {:?} binding `{name}`",
                                    binding.namespace
                                ),
                            )
                            .with_secondary(previous.span, "first bound here"),
                        ));
                    }
                }
                for binding in bindings {
                    modules[index(use_item.module)]
                        .scopes
                        .entry(binding.namespace)
                        .or_default()
                        .push(HirScopeEntry {
                            name: name.clone(),
                            definition: binding.definition,
                            kind: binding.kind,
                            namespace: binding.namespace,
                            visibility,
                            exportable: binding.exportable,
                            span: use_item.import.span,
                            imported: true,
                        });
                }
                progressed = true;
            }
            if !progressed {
                let first = &next[0];
                let path = display_path(&first.import.path);
                return Err(self.error(Diagnostic::at(
                    "NAME002",
                    first.import.path.span,
                    format!("unresolved import `{path}`"),
                )));
            }
            pending = next;
        }
        Ok(())
    }

    fn resolve_hir_path(
        &self,
        modules: &[HirModule],
        from: ModuleId,
        path: &AstPath,
    ) -> Result<Option<Vec<ResolvedBinding>>, FrontendError> {
        let mut segments = path.segments.iter();
        let mut current = match path.root {
            AstPathRoot::Package => ModuleId(0),
            AstPathRoot::SelfValue => from,
            AstPathRoot::Super(count) => self.ascend(from, count).ok_or_else(|| {
                self.error(Diagnostic::at(
                    "NAME002",
                    path.span,
                    "path uses `super` above the target root",
                ))
            })?,
            AstPathRoot::Bare => {
                let first = segments.next().expect("path has segment");
                let surface = self
                    .dependency_exports
                    .get(first.as_str())
                    .ok_or_else(|| {
                        self.error(Diagnostic::at(
                            "NAME002",
                            path.span,
                            format!(
                                "`{first}` is not a declared dependency alias; local paths must begin with `package::`, `self::`, or `super::`"
                            ),
                        ))
                    })?
                    .as_ref()
                    .ok_or_else(|| {
                        self.error(Diagnostic::at(
                            "NAME008",
                            path.span,
                            format!(
                                "dependency alias `{first}` has no checked library export surface"
                            ),
                        ))
                    })?;
                return self.resolve_dependency_path(
                    first.as_str(),
                    surface,
                    &[],
                    segments.cloned().collect::<Vec<_>>().as_slice(),
                    path.span,
                );
            }
        };

        while let Some(segment) = segments.next() {
            let matches = scope_matches(&modules[index(current)], segment);
            if matches.is_empty() {
                return Ok(None);
            }
            if segments.len() == 0 {
                return self.finish_local_path(matches, from, path.span);
            }
            let entry = self.select_local_module(matches, from, path.span, segment.as_str())?;
            current = match entry.kind {
                HirDefinitionKind::Module(module)
                    if entry.definition.package() == self.request.package
                        && entry.definition.target() == self.request.target_id =>
                {
                    module
                }
                HirDefinitionKind::Module(_) => {
                    return self.resolve_from_external_module(
                        entry.definition,
                        segments.cloned().collect::<Vec<_>>().as_slice(),
                        path.span,
                    );
                }
                _ => {
                    return Err(self.error(Diagnostic::at(
                        "NAME002",
                        path.span,
                        format!("`{segment}` is not a module"),
                    )));
                }
            };
        }
        Ok(None)
    }

    fn finish_local_path(
        &self,
        matches: Vec<&HirScopeEntry>,
        from: ModuleId,
        span: Span,
    ) -> Result<Option<Vec<ResolvedBinding>>, FrontendError> {
        for entry in &matches {
            self.require_visible(entry, from, span)?;
        }
        if matches.iter().enumerate().any(|(offset, left)| {
            matches[offset + 1..]
                .iter()
                .any(|right| left.namespace == right.namespace)
        }) {
            return Err(self.error(Diagnostic::at(
                "NAME003",
                span,
                "path resolves to multiple bindings in the same namespace",
            )));
        }
        Ok(Some(
            matches
                .into_iter()
                .map(|entry| ResolvedBinding {
                    definition: entry.definition,
                    namespace: entry.namespace,
                    kind: entry.kind,
                    visibility: entry.visibility,
                    exportable: entry.exportable,
                })
                .collect(),
        ))
    }

    fn select_local_module<'a>(
        &self,
        matches: Vec<&'a HirScopeEntry>,
        from: ModuleId,
        span: Span,
        segment: &str,
    ) -> Result<&'a HirScopeEntry, FrontendError> {
        let modules = matches
            .into_iter()
            .filter(|entry| matches!(entry.kind, HirDefinitionKind::Module(_)))
            .collect::<Vec<_>>();
        if modules.is_empty() {
            return Err(self.error(Diagnostic::at(
                "NAME002",
                span,
                format!("`{segment}` is not a module"),
            )));
        }
        if modules.len() != 1 {
            return Err(self.error(Diagnostic::at(
                "NAME003",
                span,
                format!("ambiguous module path segment `{segment}`"),
            )));
        }
        self.require_visible(modules[0], from, span)?;
        Ok(modules[0])
    }

    fn resolve_from_external_module(
        &self,
        definition: HirDefinitionId,
        remaining: &[Symbol],
        span: Span,
    ) -> Result<Option<Vec<ResolvedBinding>>, FrontendError> {
        let (alias, surface, entry) = self
            .dependency_exports
            .iter()
            .filter_map(|(alias, surface)| {
                let surface = surface.as_ref()?;
                surface
                    .entries
                    .iter()
                    .find(|entry| entry.definition == definition)
                    .map(|entry| (alias.as_str(), surface, entry))
            })
            .next()
            .ok_or_else(|| {
                self.error(Diagnostic::at(
                    "NAME008",
                    span,
                    "imported dependency module has no retained export surface",
                ))
            })?;
        self.resolve_dependency_path(alias, surface, &entry.path, remaining, span)
    }

    fn resolve_dependency_path(
        &self,
        alias: &str,
        surface: &PackageExportSurface,
        prefix: &[Symbol],
        remaining: &[Symbol],
        span: Span,
    ) -> Result<Option<Vec<ResolvedBinding>>, FrontendError> {
        if remaining.is_empty() {
            return Err(self.error(Diagnostic::at(
                "NAME002",
                span,
                format!("dependency alias `{alias}` must be followed by an exported item path"),
            )));
        }
        let mut path = prefix.to_vec();
        for (offset, segment) in remaining.iter().enumerate() {
            path.push(segment.clone());
            let matches = surface
                .entries
                .iter()
                .filter(|entry| entry.path == path)
                .collect::<Vec<_>>();
            if matches.is_empty() {
                return Ok(None);
            }
            let final_segment = offset + 1 == remaining.len();
            let candidates = if final_segment {
                if matches.iter().any(|entry| !entry.externally_visible) {
                    return Err(self.private_dependency_path(alias, &path, span));
                }
                matches
            } else {
                let modules = matches
                    .into_iter()
                    .filter(|entry| matches!(entry.kind, HirDefinitionKind::Module(_)))
                    .collect::<Vec<_>>();
                if modules.iter().any(|entry| !entry.externally_visible) {
                    return Err(self.private_dependency_path(alias, &path, span));
                }
                modules
            };
            if candidates.is_empty() {
                return Err(self.error(Diagnostic::at(
                    "NAME002",
                    span,
                    format!("`{alias}::{segment}` is not an exported module"),
                )));
            }
            if !final_segment && candidates.len() != 1 {
                return Err(self.error(Diagnostic::at(
                    "NAME003",
                    span,
                    format!("ambiguous dependency path through `{alias}::{segment}`"),
                )));
            }
            if final_segment {
                if candidates.iter().enumerate().any(|(index, left)| {
                    candidates[index + 1..]
                        .iter()
                        .any(|right| left.namespace == right.namespace)
                }) {
                    return Err(self.error(Diagnostic::at(
                        "NAME003",
                        span,
                        "dependency path resolves to multiple bindings in the same namespace",
                    )));
                }
                return Ok(Some(
                    candidates
                        .into_iter()
                        .map(|entry| ResolvedBinding {
                            definition: entry.definition,
                            namespace: entry.namespace,
                            kind: entry.kind,
                            visibility: HirVisibility::Public,
                            exportable: true,
                        })
                        .collect(),
                ));
            }
        }
        Ok(None)
    }

    fn private_dependency_path(&self, alias: &str, path: &[Symbol], span: Span) -> FrontendError {
        self.error(Diagnostic::at(
            "VISIBILITY003",
            span,
            format!(
                "`{alias}::{}` is not publicly exported",
                path.iter()
                    .map(Symbol::as_str)
                    .collect::<Vec<_>>()
                    .join("::")
            ),
        ))
    }

    fn require_visible(
        &self,
        entry: &HirScopeEntry,
        from: ModuleId,
        span: Span,
    ) -> Result<(), FrontendError> {
        if self.visibility_allows(entry.visibility, from) {
            Ok(())
        } else {
            Err(self.error(
                Diagnostic::at(
                    "VISIBILITY003",
                    span,
                    format!("`{}` is private in this module", entry.name),
                )
                .with_secondary(entry.span, "binding visibility declared here"),
            ))
        }
    }

    fn visibility_allows(&self, visibility: HirVisibility, from: ModuleId) -> bool {
        match visibility {
            HirVisibility::Public | HirVisibility::Package => true,
            HirVisibility::Module(boundary) => self.is_descendant(from, boundary),
        }
    }

    fn visibility_is_subset(&self, requested: HirVisibility, original: HirVisibility) -> bool {
        match (requested, original) {
            (_, HirVisibility::Public) => true,
            (HirVisibility::Public, _) => false,
            (HirVisibility::Package, HirVisibility::Package) => true,
            (HirVisibility::Package, HirVisibility::Module(_)) => false,
            (HirVisibility::Module(_), HirVisibility::Package) => true,
            (HirVisibility::Module(requested), HirVisibility::Module(original)) => {
                self.is_descendant(requested, original)
            }
        }
    }

    fn link_target(
        &self,
        modules: &[HirModule],
        definitions: &[HirDefinition],
    ) -> Result<LinkedTarget, FrontendError> {
        let main = modules[index(ModuleId(0))]
            .scopes
            .get(&HirNamespace::Value)
            .and_then(|entries| {
                entries.iter().find(|entry| {
                    entry.name.as_str() == "main" && entry.kind == HirDefinitionKind::Function
                })
            });
        let worlds = definitions
            .iter()
            .filter(|definition| definition.kind == HirDefinitionKind::World)
            .collect::<Vec<_>>();

        match self.request.kind {
            TargetKind::Library => {
                if let Some(world) = worlds.first() {
                    return Err(self.error(Diagnostic::at(
                        "TARGET001",
                        world.span,
                        "library targets cannot define a root world",
                    )));
                }
                if let Some(main) = main {
                    return Err(self.error(Diagnostic::at(
                        "TARGET002",
                        main.span,
                        "library targets cannot define process `main`",
                    )));
                }
                if self.request.root_world.is_some() {
                    return Err(self.error(Diagnostic::path(
                        "TARGET001",
                        "library target manifest cannot select a root world",
                    )));
                }
                Ok(LinkedTarget {
                    package: self.request.package,
                    id: self.request.target_id,
                    name: self.request.target_name.clone(),
                    kind: self.request.kind,
                    root_module: ModuleId(0),
                    root_world: None,
                    main: None,
                    reset_schedule: None,
                    step_schedule: None,
                    self_play_schedule: None,
                })
            }
            TargetKind::Binary => {
                let world = self.resolve_manifest_item(
                    modules,
                    "root world",
                    self.request.root_world.as_deref(),
                    HirDefinitionKind::World,
                )?;
                let main = main.ok_or_else(|| {
                    self.error(Diagnostic::path(
                        "TARGET003",
                        "binary target requires exactly one root-exported `pub fn main`; found 0",
                    ))
                })?;
                if main.visibility != HirVisibility::Public || !main.exportable {
                    return Err(self.error(Diagnostic::at(
                        "TARGET004",
                        main.span,
                        "binary entrypoint must be exported from the target root as `pub fn main`",
                    )));
                }
                Ok(LinkedTarget {
                    package: self.request.package,
                    id: self.request.target_id,
                    name: self.request.target_name.clone(),
                    kind: self.request.kind,
                    root_module: ModuleId(0),
                    root_world: Some(world),
                    main: Some(main.definition),
                    reset_schedule: None,
                    step_schedule: None,
                    self_play_schedule: None,
                })
            }
            TargetKind::Environment => {
                if let Some(main) = main {
                    return Err(self.error(Diagnostic::at(
                        "TARGET005",
                        main.span,
                        "environment targets cannot define process `main`",
                    )));
                }
                let world = self.resolve_manifest_item(
                    modules,
                    "root world",
                    self.request.root_world.as_deref(),
                    HirDefinitionKind::World,
                )?;
                let profile = self.request.environment_schedules.as_ref().ok_or_else(|| {
                    self.error(Diagnostic::path(
                        "TARGET006",
                        "environment target requires reset, step, and self-play schedule paths",
                    ))
                })?;
                let reset = self.resolve_manifest_item(
                    modules,
                    "reset schedule",
                    Some(&profile.reset),
                    HirDefinitionKind::Schedule,
                )?;
                let step = self.resolve_manifest_item(
                    modules,
                    "step schedule",
                    Some(&profile.step),
                    HirDefinitionKind::Schedule,
                )?;
                let self_play = self.resolve_manifest_item(
                    modules,
                    "self-play schedule",
                    Some(&profile.self_play),
                    HirDefinitionKind::Schedule,
                )?;
                Ok(LinkedTarget {
                    package: self.request.package,
                    id: self.request.target_id,
                    name: self.request.target_name.clone(),
                    kind: self.request.kind,
                    root_module: ModuleId(0),
                    root_world: Some(world),
                    main: None,
                    reset_schedule: Some(reset),
                    step_schedule: Some(step),
                    self_play_schedule: Some(self_play),
                })
            }
        }
    }

    fn resolve_manifest_item(
        &self,
        modules: &[HirModule],
        label: &str,
        path: Option<&str>,
        expected: HirDefinitionKind,
    ) -> Result<HirDefinitionId, FrontendError> {
        let raw = path.ok_or_else(|| {
            self.error(Diagnostic::path(
                "TARGET007",
                format!("target manifest must explicitly name its {label}"),
            ))
        })?;
        let segments = manifest_path(raw).map_err(|message| {
            self.error(Diagnostic::path(
                "TARGET008",
                format!("invalid {label} path `{raw}`: {message}"),
            ))
        })?;
        let mut module = ModuleId(0);
        for segment in &segments[..segments.len() - 1] {
            let matches = modules[index(module)]
                .scopes
                .get(&HirNamespace::Module)
                .into_iter()
                .flatten()
                .filter(|entry| entry.name.as_str() == segment)
                .collect::<Vec<_>>();
            if matches.len() != 1 {
                return Err(self.error(Diagnostic::path(
                    "TARGET008",
                    format!(
                        "module `{segment}` in {label} path `{raw}` resolved to {} bindings",
                        matches.len()
                    ),
                )));
            }
            let binding = matches[0];
            if !self.visibility_allows(binding.visibility, ModuleId(0)) {
                return Err(self.error(Diagnostic::at(
                    "VISIBILITY003",
                    binding.span,
                    format!("module `{segment}` in {label} path `{raw}` is not visible from the target root"),
                )));
            }
            let HirDefinitionKind::Module(next) = binding.kind else {
                return Err(self.error(Diagnostic::at(
                    "TARGET009",
                    binding.span,
                    format!("`{segment}` in {label} path `{raw}` is not a module"),
                )));
            };
            if binding.definition.package() != self.request.package
                || binding.definition.target() != self.request.target_id
            {
                return Err(self.error(Diagnostic::at(
                    "TARGET008",
                    binding.span,
                    format!("{label} path `{raw}` traverses a module outside this target"),
                )));
            }
            module = next;
        }
        let name = segments.last().expect("manifest path is non-empty");
        let matches = modules[index(module)]
            .scopes
            .get(&expected.namespace())
            .into_iter()
            .flatten()
            .filter(|entry| entry.name.as_str() == name)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(self.error(Diagnostic::path(
                "TARGET008",
                format!(
                    "{label} path `{raw}` resolved to {} definitions",
                    matches.len()
                ),
            )));
        }
        let binding = matches[0];
        if !self.visibility_allows(binding.visibility, ModuleId(0)) {
            return Err(self.error(Diagnostic::at(
                "VISIBILITY003",
                binding.span,
                format!("{label} `{raw}` is not visible from the target root"),
            )));
        }
        if binding.kind != expected {
            return Err(self.error(Diagnostic::at(
                "TARGET009",
                binding.span,
                format!("{label} `{raw}` has the wrong item kind"),
            )));
        }
        Ok(binding.definition)
    }

    fn ascend(&self, mut module: ModuleId, count: u64) -> Option<ModuleId> {
        for _ in 0..count {
            module = self.modules.get(index(module))?.parent?;
        }
        Some(module)
    }

    fn is_descendant(&self, mut module: ModuleId, boundary: ModuleId) -> bool {
        loop {
            if module == boundary {
                return true;
            }
            let Some(parent) = self.modules[index(module)].parent else {
                return false;
            };
            module = parent;
        }
    }

    fn error(&self, diagnostic: Diagnostic) -> FrontendError {
        FrontendError {
            kind: classify(diagnostic.code),
            diagnostic: Box::new(diagnostic),
            files: self.files.clone(),
        }
    }
}

#[derive(Clone)]
struct PendingUse {
    module: ModuleId,
    import: AstUse,
}

#[derive(Clone, Copy)]
struct ResolvedBinding {
    definition: HirDefinitionId,
    namespace: HirNamespace,
    kind: HirDefinitionKind,
    visibility: HirVisibility,
    exportable: bool,
}

fn hir_kind(kind: AstDefinitionKind) -> HirDefinitionKind {
    match kind {
        AstDefinitionKind::World => HirDefinitionKind::World,
        AstDefinitionKind::Component => HirDefinitionKind::Component,
        AstDefinitionKind::Resource => HirDefinitionKind::Resource,
        AstDefinitionKind::Tag => HirDefinitionKind::Tag,
        AstDefinitionKind::System => HirDefinitionKind::System,
        AstDefinitionKind::Schedule => HirDefinitionKind::Schedule,
        AstDefinitionKind::Function => HirDefinitionKind::Function,
        AstDefinitionKind::Struct => HirDefinitionKind::Struct,
        AstDefinitionKind::Enum => HirDefinitionKind::Enum,
        AstDefinitionKind::Trait => HirDefinitionKind::Trait,
        AstDefinitionKind::TypeAlias => HirDefinitionKind::TypeAlias,
        AstDefinitionKind::Const => HirDefinitionKind::Const,
        AstDefinitionKind::Static => HirDefinitionKind::Static,
    }
}

fn scope_matches<'a>(module: &'a HirModule, name: &Symbol) -> Vec<&'a HirScopeEntry> {
    module
        .scopes
        .values()
        .flat_map(|entries| entries.iter())
        .filter(|entry| entry.name == *name)
        .collect()
}

fn manifest_path(raw: &str) -> Result<Vec<String>, &'static str> {
    let raw = raw.strip_prefix("package::").unwrap_or(raw);
    if raw.is_empty() {
        return Err("path has no item segment");
    }
    raw.split("::")
        .map(|segment| {
            if segment.is_empty() {
                return Err("path contains an empty segment");
            }
            normalize_identifier(segment).map_err(|_| "path contains an invalid identifier")
        })
        .collect()
}

fn display_path(path: &AstPath) -> String {
    let root = match path.root {
        AstPathRoot::Bare => String::new(),
        AstPathRoot::Package => "package::".to_owned(),
        AstPathRoot::SelfValue => "self::".to_owned(),
        AstPathRoot::Super(count) => "super::".repeat(usize::try_from(count).unwrap_or(usize::MAX)),
    };
    format!(
        "{root}{}",
        path.segments
            .iter()
            .map(Symbol::as_str)
            .collect::<Vec<_>>()
            .join("::")
    )
}

fn classify(code: &str) -> FrontendErrorCode {
    if code.starts_with("SOURCE") {
        FrontendErrorCode::Source
    } else if code.starts_with("PARSE") || code.starts_with("LEX") {
        FrontendErrorCode::Syntax
    } else if code.starts_with("MODULE") {
        FrontendErrorCode::Module
    } else if code.starts_with("NAME") {
        FrontendErrorCode::Name
    } else if code.starts_with("VISIBILITY") {
        FrontendErrorCode::Visibility
    } else if code.starts_with("TARGET") {
        FrontendErrorCode::Target
    } else {
        FrontendErrorCode::Migration
    }
}

fn standalone_error(code: &'static str, message: impl Into<String>) -> FrontendError {
    let diagnostic = Diagnostic::path(code, message);
    FrontendError {
        kind: classify(code),
        diagnostic: Box::new(diagnostic),
        files: Vec::new(),
    }
}

fn resolve_target_source(
    package_root: &Path,
    declared_package_root: &Path,
    declared_source: &Path,
) -> Result<PathBuf, FrontendError> {
    let relative = if declared_source.is_absolute() {
        if let Ok(relative) = declared_source.strip_prefix(declared_package_root) {
            relative.to_path_buf()
        } else {
            let canonical = fs::canonicalize(declared_source).map_err(|error| {
                standalone_error(
                    "SOURCE001",
                    format!(
                        "could not resolve target source {}: {error}",
                        declared_source.display()
                    ),
                )
            })?;
            canonical
                .strip_prefix(package_root)
                .map(Path::to_path_buf)
                .map_err(|_| {
                    standalone_error(
                        "MODULE005",
                        format!(
                            "target source {} resolves outside package root {}",
                            declared_source.display(),
                            package_root.display()
                        ),
                    )
                })?
        }
    } else {
        declared_source.to_path_buf()
    };
    if relative.file_name().is_some_and(|name| name == "mod.arc") {
        return Err(standalone_error(
            "MODULE003",
            "`mod.arc` is not a target or child-module source name",
        ));
    }

    let mut current = package_root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(expected_os) = component else {
            return Err(standalone_error(
                "MODULE005",
                format!(
                    "target source path {} is not package-relative",
                    declared_source.display()
                ),
            ));
        };
        let expected = expected_os.to_str().ok_or_else(|| {
            standalone_error(
                "MODULE002",
                format!(
                    "target source path {} contains a non-UTF-8 name",
                    declared_source.display()
                ),
            )
        })?;
        let expected_nfc =
            unicode_normalization::UnicodeNormalization::nfc(expected).collect::<String>();
        if expected_nfc != expected {
            return Err(standalone_error(
                "MODULE002",
                format!(
                    "target source path component `{expected}` is not NFC-normalized; use `{expected_nfc}`"
                ),
            ));
        }
        let expected_fold = case_fold_nfc(expected);
        let entries = fs::read_dir(&current).map_err(|error| {
            standalone_error(
                "SOURCE001",
                format!(
                    "could not inspect target path {}: {error}",
                    current.display()
                ),
            )
        })?;
        let mut aliases = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                standalone_error(
                    "SOURCE001",
                    format!(
                        "could not enumerate target path {}: {error}",
                        current.display()
                    ),
                )
            })?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if unicode_normalization::UnicodeNormalization::nfc(name.as_str()).collect::<String>()
                == expected_nfc
                || case_fold_nfc(&name) == expected_fold
            {
                aliases.push(name);
            }
        }
        aliases.sort();
        if aliases.len() != 1 || aliases[0] != expected {
            let detail = if aliases.is_empty() {
                format!(
                    "no exact entry named `{expected}` exists in {}",
                    current.display()
                )
            } else {
                format!(
                    "target path component `{expected}` has case/NFC aliases in {}: {}",
                    current.display(),
                    aliases.join(", ")
                )
            };
            return Err(standalone_error("MODULE002", detail));
        }
        current.push(expected);
    }
    let canonical = fs::canonicalize(&current).map_err(|error| {
        standalone_error(
            "SOURCE001",
            format!(
                "could not resolve target source {}: {error}",
                current.display()
            ),
        )
    })?;
    if !canonical.starts_with(package_root) {
        return Err(standalone_error(
            "MODULE005",
            format!(
                "target source {} resolves outside package root {}",
                canonical.display(),
                package_root.display()
            ),
        ));
    }
    // Keep the exact declared path for the secure open. `load_module` binds this
    // path to the checked canonical destination and rejects link/reparse aliases.
    Ok(current)
}

fn to_u64(value: usize, context: &'static str) -> Result<u64, Diagnostic> {
    u64::try_from(value)
        .map_err(|_| Diagnostic::path("SOURCE003", format!("{context} exceeds u64")))
}

fn index<T>(id: T) -> usize
where
    T: IntoIndex,
{
    usize::try_from(id.into_u64()).expect("dense HIR id originated from usize")
}

trait IntoIndex {
    fn into_u64(self) -> u64;
}

impl IntoIndex for ModuleId {
    fn into_u64(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dump_resolved_target;
    use crate::source::test_support::unique_directory;
    use std::fs;

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = unique_directory(name);
            fs::create_dir_all(root.join("src")).unwrap();
            Self { root }
        }

        fn write(&self, relative: &str, source: &str) {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, source).unwrap();
        }

        fn request(&self, kind: TargetKind) -> CheckTargetRequest {
            CheckTargetRequest {
                package_root: self.root.clone(),
                package: arche_package::PackageNodeId::new(0),
                target_id: crate::TargetId(0),
                target_name: "fixture".to_owned(),
                kind,
                source_root: PathBuf::from("src/main.arc"),
                root_world: None,
                environment_schedules: None,
                dependency_aliases: Vec::new(),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn loads_only_explicit_modules_with_nested_mapping() {
        let fixture = Fixture::new("explicit-modules");
        fixture.write(
            "src/main.arc",
            "mod physics; pub world Game { init { } } pub fn main() { }",
        );
        fixture.write(
            "src/physics.arc",
            "pub mod collision; pub component Body { }",
        );
        fixture.write("src/physics/collision.arc", "pub component Hit { }");
        fixture.write("src/ignored.arc", "startup { exit 1 }");
        let mut request = fixture.request(TargetKind::Binary);
        request.root_world = Some("package::Game".to_owned());

        let hir = check_target(request).unwrap();
        assert_eq!(hir.modules().len(), 3);
        assert_eq!(hir.source_entries().len(), 3);
        assert!(hir
            .source_entries()
            .iter()
            .all(|entry| entry.path.as_str() != "src/ignored.arc"));
    }

    #[test]
    fn source_entries_commit_the_checked_snapshots_not_reopened_paths() {
        let fixture = Fixture::new("immutable-source-entry");
        fixture.write(
            "src/main.arc",
            "pub world Game { init { } } pub fn main() { }",
        );
        let mut request = fixture.request(TargetKind::Binary);
        request.root_world = Some("package::Game".to_owned());

        let hir = check_target(request.clone()).unwrap();
        let retained = hir.source_entries()[0].clone();
        assert_eq!(retained.path.as_str(), "src/main.arc");

        fixture.write(
            "src/main.arc",
            "pub world Changed { init { } } pub fn main() { }",
        );
        assert_eq!(hir.source_entries()[0], retained);
        request.root_world = Some("package::Changed".to_owned());
        let changed = check_target(request).unwrap();
        assert_ne!(changed.source_entries()[0], retained);
    }

    #[test]
    fn resolves_imports_into_ordered_namespaces_and_dumps_deterministically() {
        let fixture = Fixture::new("resolved-hir");
        fixture.write(
            "src/main.arc",
            "pub mod shared; use package::shared::Position; pub world Game { init { } } pub fn main() { }",
        );
        fixture.write("src/shared.arc", "pub component Position { x: i32 }");
        let mut request = fixture.request(TargetKind::Binary);
        request.root_world = Some("package::Game".to_owned());

        let first = check_target(request.clone()).unwrap();
        let second = check_target(request).unwrap();
        assert_eq!(dump_resolved_target(&first), dump_resolved_target(&second));
        assert!(dump_resolved_target(&first).contains("Type Position -> p0t0d"));
    }

    #[test]
    fn one_use_imports_distinct_type_and_value_namespace_bindings() {
        let fixture = Fixture::new("multi-namespace-use");
        fixture.write("src/main.arc", "mod shared; mod consumer;");
        fixture.write("src/shared.arc", "pub struct Both { } pub fn Both() { }");
        fixture.write(
            "src/consumer.arc",
            "use package::shared::Both; pub component Marker { }",
        );

        let hir = check_target(fixture.request(TargetKind::Library)).unwrap();
        let consumer = hir
            .modules()
            .iter()
            .find(|module| {
                module
                    .path
                    .last()
                    .is_some_and(|name| name.as_str() == "consumer")
            })
            .unwrap();
        for namespace in [HirNamespace::Type, HirNamespace::Value] {
            assert!(consumer.scopes[&namespace]
                .iter()
                .any(|entry| entry.imported && entry.name.as_str() == "Both"));
        }
    }

    #[test]
    fn enforces_target_world_main_and_environment_schedule_links() {
        let fixture = Fixture::new("target-links");
        fixture.write(
            "src/main.arc",
            "pub world Grid { init { } } pub schedule Reset { } pub schedule Step { } pub schedule SelfPlay { }",
        );
        let mut request = fixture.request(TargetKind::Environment);
        request.root_world = Some("package::Grid".to_owned());
        request.environment_schedules = Some(crate::EnvironmentSchedulePaths {
            reset: "package::Reset".to_owned(),
            step: "package::Step".to_owned(),
            self_play: "package::SelfPlay".to_owned(),
        });
        let hir = check_target(request).unwrap();
        assert!(hir.target().root_world.is_some());
        assert!(hir.target().reset_schedule.is_some());
    }

    #[test]
    fn target_links_resolve_root_reexports_in_the_expected_namespace() {
        let fixture = Fixture::new("target-scope-links");
        fixture.write(
            "src/main.arc",
            "mod hidden; pub use self::hidden::Game; pub use self::hidden::main;",
        );
        fixture.write(
            "src/hidden.arc",
            "pub world Game { init { } } pub fn Game() { } pub fn main() { }",
        );
        let mut request = fixture.request(TargetKind::Binary);
        request.root_world = Some("package::Game".to_owned());

        let hir = check_target(request).unwrap();
        assert!(hir.target().root_world.is_some());
        assert!(hir.target().main.is_some());
        assert_ne!(
            hir.target().root_world.unwrap(),
            hir.modules()[0].scopes[&HirNamespace::Value]
                .iter()
                .find(|entry| entry.name.as_str() == "Game")
                .unwrap()
                .definition,
            "the manifest's world context must ignore the same-name value binding"
        );
    }

    #[test]
    fn hidden_child_main_is_not_a_binary_root_export() {
        let fixture = Fixture::new("hidden-main");
        fixture.write("src/main.arc", "mod hidden; pub world Game { init { } }");
        fixture.write("src/hidden.arc", "pub fn main() { }");
        let mut request = fixture.request(TargetKind::Binary);
        request.root_world = Some("package::Game".to_owned());

        assert_eq!(
            check_target(request).unwrap_err().diagnostic.code,
            "TARGET003"
        );
    }

    #[test]
    fn library_rejects_world_and_binary_requires_public_main() {
        let fixture = Fixture::new("target-errors");
        fixture.write("src/main.arc", "world Game { init { } } fn main() { }");
        assert_eq!(
            check_target(fixture.request(TargetKind::Library))
                .unwrap_err()
                .diagnostic
                .code,
            "TARGET001"
        );
        let mut binary = fixture.request(TargetKind::Binary);
        binary.root_world = Some("Game".to_owned());
        assert_eq!(
            check_target(binary).unwrap_err().diagnostic.code,
            "TARGET004"
        );
    }

    #[test]
    fn rejects_exact_case_and_nfc_filename_mismatch() {
        let fixture = Fixture::new("case-mismatch");
        fixture.write(
            "src/main.arc",
            "mod physics; pub world Game { init { } } pub fn main() { }",
        );
        fixture.write("src/Physics.arc", "pub component Body { }");
        let mut request = fixture.request(TargetKind::Binary);
        request.root_world = Some("Game".to_owned());
        assert_eq!(
            check_target(request).unwrap_err().diagnostic.code,
            "MODULE002"
        );
    }

    #[test]
    fn rejects_wrong_case_nested_module_directories_on_every_host() {
        let fixture = Fixture::new("nested-directory-case");
        fixture.write("src/main.arc", "mod physics;");
        fixture.write("src/physics.arc", "pub mod collision;");
        fixture.write("src/Physics/collision.arc", "pub component Hit { }");

        assert_eq!(
            check_target(fixture.request(TargetKind::Library))
                .unwrap_err()
                .diagnostic
                .code,
            "MODULE002"
        );
    }

    #[test]
    fn rejects_wrong_case_and_nfc_aliases_for_the_manifest_target_root() {
        let wrong_case = Fixture::new("target-root-case");
        wrong_case.write("src/Main.arc", "pub component Marker { }");
        assert_eq!(
            check_target(wrong_case.request(TargetKind::Library))
                .unwrap_err()
                .diagnostic
                .code,
            "MODULE002"
        );

        let nfc = Fixture::new("target-root-nfc");
        nfc.write("src/caf\u{e9}.arc", "pub component Marker { }");
        nfc.write("src/cafe\u{301}.arc", "pub component Alias { }");
        let mut request = nfc.request(TargetKind::Library);
        request.source_root = PathBuf::from("src/caf\u{e9}.arc");
        assert_eq!(
            check_target(request).unwrap_err().diagnostic.code,
            "MODULE002"
        );
    }

    #[test]
    fn resolves_super_and_pub_in_boundaries_and_rejects_non_ancestors() {
        let fixture = Fixture::new("visibility-boundaries");
        fixture.write("src/main.arc", "mod outer;");
        fixture.write(
            "src/outer.arc",
            "pub mod child; pub(super) component FromSuper { } pub(in package::outer) component InOuter { }",
        );
        fixture.write(
            "src/outer/child.arc",
            "use super::FromSuper; use super::InOuter; pub component Child { }",
        );
        assert!(check_target(fixture.request(TargetKind::Library)).is_ok());

        let invalid = Fixture::new("visibility-non-ancestor");
        invalid.write("src/main.arc", "mod outer; mod sibling;");
        invalid.write(
            "src/outer.arc",
            "pub(in package::sibling) component Bad { }",
        );
        invalid.write("src/sibling.arc", "pub component Marker { }");
        assert_eq!(
            check_target(invalid.request(TargetKind::Library))
                .unwrap_err()
                .diagnostic
                .code,
            "VISIBILITY002"
        );
    }

    #[test]
    fn local_import_and_visibility_paths_require_explicit_roots() {
        let import = Fixture::new("bare-local-import");
        import.write("src/main.arc", "mod local; use local::Thing;");
        import.write("src/local.arc", "pub component Thing { }");
        let error = check_target(import.request(TargetKind::Library)).unwrap_err();
        assert_eq!(error.diagnostic.code, "NAME002");
        assert!(error
            .diagnostic
            .message
            .contains("not a declared dependency alias"));

        let visibility = Fixture::new("bare-visibility-path");
        visibility.write(
            "src/main.arc",
            "mod local; pub(in local) component Hidden { }",
        );
        visibility.write("src/local.arc", "pub component Marker { }");
        let error = check_target(visibility.request(TargetKind::Library)).unwrap_err();
        assert_eq!(error.diagnostic.code, "NAME002");
        assert!(error
            .diagnostic
            .message
            .contains("must begin with `package::`, `self::`, or `super::`"));
    }

    #[test]
    fn rejects_package_visibility_that_widens_a_module_private_import() {
        let fixture = Fixture::new("visibility-widening");
        fixture.write("src/main.arc", "mod outer; mod sibling;");
        fixture.write("src/outer.arc", "pub mod child; component Secret { }");
        fixture.write("src/outer/child.arc", "pub(package) use super::Secret;");
        fixture.write("src/sibling.arc", "pub component Marker { }");

        assert_eq!(
            check_target(fixture.request(TargetKind::Library))
                .unwrap_err()
                .diagnostic
                .code,
            "VISIBILITY004"
        );
    }

    #[test]
    fn dependency_aliases_reject_full_casefold_collisions() {
        let fixture = Fixture::new("dependency-alias-collision");
        fixture.write("src/main.arc", "pub component Marker { }");
        let mut request = fixture.request(TargetKind::Library);
        request.dependency_aliases = vec!["Physics".to_owned(), "physics".to_owned()];
        assert_eq!(
            check_target(request).unwrap_err().diagnostic.code,
            "NAME007"
        );
    }

    #[test]
    fn rejects_physical_source_aliases() {
        let fixture = Fixture::new("physical-alias");
        fixture.write(
            "src/main.arc",
            "mod a; mod b; pub world Game { init { } } pub fn main() { }",
        );
        fixture.write("src/a.arc", "pub component Shared { }");
        fs::hard_link(
            fixture.root.join("src/a.arc"),
            fixture.root.join("src/b.arc"),
        )
        .unwrap();
        let mut request = fixture.request(TargetKind::Binary);
        request.root_world = Some("Game".to_owned());
        assert_eq!(
            check_target(request).unwrap_err().diagnostic.code,
            "MODULE007"
        );
    }

    #[test]
    fn migration_diagnostic_survives_module_loading() {
        let fixture = Fixture::new("migration");
        fixture.write("src/main.arc", "startup { exit 0 }");
        assert_eq!(
            check_target(fixture.request(TargetKind::Library))
                .unwrap_err()
                .diagnostic
                .code,
            "MIGRATE001"
        );
    }
}
