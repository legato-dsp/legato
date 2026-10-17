#include "tree_sitter/parser.h"
#include <napi.h>

typedef struct TSLanguage TSLanguage;

extern "C" TSLanguage *tree_sitter_legato();

namespace {

Napi::Object Init(Napi::Env env, Napi::Object exports) {
  exports["name"] = Napi::String::New(env, "legato");
  auto language = Napi::External<TSLanguage>::New(env, tree_sitter_legato());
  language.TypeTag(&exports.TypeTag);
  exports["language"] = language;
  return exports;
}

} // namespace

NODE_API_MODULE(tree_sitter_legato_binding, Init)
