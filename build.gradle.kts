import com.github.jengelman.gradle.plugins.shadow.tasks.ShadowJar

fun getVersionName(tagName: String) = if(tagName.startsWith("v")) tagName.substring(1) else tagName
val gitTagName: String? get() = Regex("(?<=refs/tags/).*").find(System.getenv("GITHUB_REF") ?: "")?.value
val debugVersion: String get() = System.getenv("DBG_VERSION") ?: "0"

group = "com.github.balloonupdate"
version = gitTagName?.run { getVersionName(this) } ?: debugVersion

repositories {
    mavenCentral()
}

dependencies {
//    implementation("net.java.dev.jna:jna:5.14.0")
}

plugins {
    id("java")
    id("com.github.johnrengelman.shadow") version "7.1.2"
}

tasks.withType<ShadowJar> {
    duplicatesStrategy = DuplicatesStrategy.INCLUDE

    archiveBaseName = "loader"

    manifest {
        attributes("Version" to archiveVersion.get())
        attributes("Main-Class" to "mcpatch.Mcpatch2Loader")
        attributes("Premain-Class" to "mcpatch.Mcpatch2Loader")
    }

    archiveClassifier.set("")
}

tasks.register<JavaExec>("selfUpdateRegression") {
    dependsOn(tasks.testClasses)
    classpath = sourceSets.test.get().runtimeClasspath
    mainClass.set("mcpatch.SelfUpdateRegression")
}
