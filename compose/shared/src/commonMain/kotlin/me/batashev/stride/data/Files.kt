package me.batashev.stride.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** Whether a [FileEntry] is a folder or a leaf file. */
@Serializable
enum class FileKind {
    @SerialName("directory") Directory,
    @SerialName("file") File,
}

/** One row in a directory listing. Mirrors `GET /api/files`. */
@Serializable
data class FileEntry(
    val name: String,
    val path: String,
    val kind: FileKind,
    val size: Long? = null,
    @SerialName("updated_at") val updatedAt: Long = 0,
    @SerialName("mime_type") val mimeType: String? = null,
) {
    val isDirectory: Boolean get() = kind == FileKind.Directory
}

/** Contents of one directory: its cleaned [path] and the entries within it. */
@Serializable
data class FileListing(val path: String, val entries: List<FileEntry>)
