#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

typedef struct CssError CssError;

typedef struct StyleSheet StyleSheet;

typedef struct Targets {
  uint32_t android;
  uint32_t chrome;
  uint32_t edge;
  uint32_t firefox;
  uint32_t ie;
  uint32_t ios_saf;
  uint32_t opera;
  uint32_t safari;
  uint32_t samsung;
} Targets;

typedef struct ParseOptions {
  const char *filename;
  bool nesting;
  bool custom_media;
  bool scope;
  bool css_modules;
  const char *css_modules_pattern;
  bool css_modules_dashed_idents;
  bool error_recovery;
} ParseOptions;

typedef struct TransformOptions {
  struct Targets targets;
  char **unused_symbols;
  uintptr_t unused_symbols_len;
} TransformOptions;

typedef struct RawString {
  char *text;
  uintptr_t len;
} RawString;

typedef enum CssModuleReference_Tag {
  /**
   * A local reference.
   */
  CssModuleReference_Local,
  /**
   * A global reference.
   */
  CssModuleReference_Global,
  /**
   * A reference to an export in a different file.
   */
  CssModuleReference_Dependency,
} CssModuleReference_Tag;

typedef struct CssModuleReference_Local_Body {
  /**
   * The local (compiled) name for the reference.
   */
  struct RawString name;
} CssModuleReference_Local_Body;

typedef struct CssModuleReference_Global_Body {
  /**
   * The referenced global name.
   */
  struct RawString name;
} CssModuleReference_Global_Body;

typedef struct CssModuleReference_Dependency_Body {
  /**
   * The name to reference within the dependency.
   */
  struct RawString name;
  /**
   * The dependency specifier for the referenced file.
   */
  struct RawString specifier;
} CssModuleReference_Dependency_Body;

typedef struct CssModuleReference {
  CssModuleReference_Tag tag;
  union {
    CssModuleReference_Local_Body local;
    CssModuleReference_Global_Body global;
    CssModuleReference_Dependency_Body dependency;
  };
} CssModuleReference;

typedef struct CssModuleExport {
  struct RawString exported;
  struct RawString local;
  bool is_referenced;
  struct CssModuleReference *composes;
  uintptr_t composes_len;
} CssModuleExport;

typedef struct CssModulePlaceholder {
  struct RawString placeholder;
  struct CssModuleReference reference;
} CssModulePlaceholder;

typedef struct ToCssResult {
  struct RawString code;
  struct RawString map;
  struct CssModuleExport *exports;
  uintptr_t exports_len;
  struct CssModulePlaceholder *references;
  uintptr_t references_len;
} ToCssResult;

typedef struct PseudoClasses {
  const char *hover;
  const char *active;
  const char *focus;
  const char *focus_visible;
  const char *focus_within;
} PseudoClasses;

typedef struct ToCssOptions {
  bool minify;
  bool source_map;
  const char *input_source_map;
  uintptr_t input_source_map_len;
  const char *project_root;
  struct Targets targets;
  bool analyze_dependencies;
  struct PseudoClasses pseudo_classes;
} ToCssOptions;

bool lightningcss_browserslist_to_targets(const char *query,
                                          struct Targets *targets,
                                          struct CssError **error);

struct StyleSheet *lightningcss_stylesheet_parse(const char *source,
                                                 uintptr_t len,
                                                 struct ParseOptions options,
                                                 struct CssError **error);

bool lightningcss_stylesheet_transform(struct StyleSheet *stylesheet,
                                       struct TransformOptions options,
                                       struct CssError **error);

struct ToCssResult lightningcss_stylesheet_to_css(struct StyleSheet *stylesheet,
                                                  struct ToCssOptions options,
                                                  struct CssError **error);

void lightningcss_stylesheet_free(struct StyleSheet *stylesheet);

void lightningcss_to_css_result_free(struct ToCssResult result);

const char *lightningcss_error_message(struct CssError *error);

void lightningcss_error_free(struct CssError *error);

uintptr_t lightningcss_stylesheet_get_warning_count(struct StyleSheet *stylesheet);

const char *lightningcss_stylesheet_get_warning(struct StyleSheet *stylesheet, uintptr_t index);
