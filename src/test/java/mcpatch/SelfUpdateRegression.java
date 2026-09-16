package mcpatch;

import java.nio.file.Files;
import java.nio.file.Path;
import java.io.IOException;
import java.util.List;

public final class SelfUpdateRegression {
    public static void main(String[] args) throws Exception {
        Path dir = Files.createTempDirectory("mcupdate-loader-regression-");
        try {
            Path jar = Files.createFile(dir.resolve("Loader.jar"));
            Path fallback = Files.createFile(dir.resolve("AutoUpdateClient.exe"));
            Path older = Files.createFile(dir.resolve("AutoUpdateClient-old.exe"));

            List<java.io.File> candidates = Mcpatch2Loader.executableCandidates(
                    List.of(
                            "AutoUpdateClient-new.exe",
                            "AutoUpdateClient.exe",
                            "AutoUpdateClient-old.exe",
                            "../outside.exe",
                            "notes.txt"),
                    dir.resolve("startlist.txt").toString(),
                    jar.toFile());

            if (candidates.size() != 3)
                throw new AssertionError("unsafe entries were not rejected");
            if (candidates.get(0).isFile())
                throw new AssertionError("missing newest candidate unexpectedly exists");
            if (!candidates.get(1).toPath().equals(fallback))
                throw new AssertionError("fallback order changed");

            // Before a successful child exit no cleanup is invoked, so both
            // the fallback and older version must remain available.
            if (!Files.exists(fallback) || !Files.exists(older))
                throw new AssertionError("candidate was removed before success");

            Mcpatch2Loader.removeOtherExecutables(candidates, fallback.toFile());
            if (!Files.exists(fallback) || Files.exists(older))
                throw new AssertionError("post-success executable cleanup failed");

            IOException sideBySide = new IOException("CreateProcess error=14001, invalid XML syntax");
            if (!Mcpatch2Loader.isPossibleSecuritySoftwareInterference(sideBySide))
                throw new AssertionError("Windows 14001 was not recognized");
            if (Mcpatch2Loader.isPossibleSecuritySoftwareInterference(new IOException("CreateProcess error=2")))
                throw new AssertionError("ordinary launch failure was misclassified");

            String hint = Mcpatch2Loader.securitySoftwareHint(
                    dir.resolve("AutoUpdateClient-new.exe").toFile(),
                    List.of(sideBySide.getMessage()));
            if (!hint.contains("杀毒软件")
                    || !hint.contains("github.com/BalloonUpdate/Mcpatch2RustClient")
                    || !hint.contains("AutoUpdateClient-new.exe"))
                throw new AssertionError("security software hint lost actionable details");

            System.out.println("SELF_UPDATE_REGRESSION_OK");
        } finally {
            try (var paths = Files.walk(dir)) {
                paths.sorted(java.util.Comparator.reverseOrder()).forEach(path -> {
                    try {
                        Files.deleteIfExists(path);
                    } catch (Exception ignored) {
                    }
                });
            }
        }
    }
}
