package mcpatch;

import java.nio.file.Files;
import java.nio.file.Path;
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
