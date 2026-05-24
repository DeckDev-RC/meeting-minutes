pub(super) fn normalize_participant_names(participant_names: Option<&[String]>) -> Vec<String> {
    let mut names = Vec::new();

    for name in participant_names.unwrap_or(&[]) {
        let trimmed = name.trim();
        if trimmed.is_empty() || names.iter().any(|existing| existing == trimmed) {
            continue;
        }
        names.push(trimmed.to_string());
    }

    names
}

pub(super) fn participant_names_prompt_section(participant_names: &[String]) -> String {
    if participant_names.is_empty() {
        return String::new();
    }

    format!(
        "\nNOMES INFORMADOS PELO USUARIO:\n{}\n\nREGRAS PARA NOMES:\n- Use esta lista como fonte preferencial para nomes de participantes e responsaveis.\n- Corrija variacoes foneticas ou de transcricao quando forem claramente compativeis com a lista.\n- Nao invente nomes fora da lista. Se nao houver confianca, mantenha o campo vazio ou o rotulo Falante N.\n",
        participant_names.join(", ")
    )
}
