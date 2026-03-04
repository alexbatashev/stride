load("@rules_cc//cc:defs.bzl", "cc_library")

cc_library(
    name = "quickjs",
    srcs = [
        "dtoa.c",
        "libregexp.c",
        "libunicode.c",
        "quickjs.c",
    ],
    hdrs = glob(["*.h"]),
    copts = [
        "-DQUICKJS_NG_BUILD",
        "-D_GNU_SOURCE",
    ],
    includes = ["."],
    visibility = ["//visibility:public"],
)
