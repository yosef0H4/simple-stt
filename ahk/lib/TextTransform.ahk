SimpleSttTransformTranscript(text, removePunctuation := false, lowercaseOutput := false) {
    if removePunctuation
        text := RegExReplace(text, "\p{P}", "")
    if lowercaseOutput
        text := StrLower(text)
    return text
}
