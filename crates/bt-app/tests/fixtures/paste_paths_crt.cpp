// T-PASTE-1 / review 5 correction 4: a defined MSVC CRT consumer, not a shell or REPL.
// Compile manually with MSVC when the coordinator grants a lane. The acceptance harness must set
// lpApplicationName to this executable and lpCommandLine to a quoted program token, one space,
// then the exact literal produced by shell_literal::Encoder. The tested value is argv[1].
// The ignored direct_crt_receives_the_exact_literal_after_the_program_token Rust fixture starts
// this consumer only when explicitly selected; no Rust test compiles it.
#include <cstdint>
#include <cstdio>

int wmain(int argc, wchar_t** argv) {
    std::printf("argc=%d\n", argc);
    if (argc != 2) {
        return 2;
    }
    // Hex code units keep spaces, percent signs and trailing backslashes observable without
    // depending on a console code page, output escaping, or newline conventions in the filename.
    std::printf("argv[1]=");
    for (const wchar_t* unit = argv[1]; *unit != L'\0'; ++unit) {
        std::printf("%04X", static_cast<unsigned int>(static_cast<std::uint16_t>(*unit)));
    }
    std::printf("\n");
    return 0;
}
